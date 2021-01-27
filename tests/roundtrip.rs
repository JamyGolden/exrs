extern crate exr;

extern crate smallvec;

use std::{panic};
use std::io::{Cursor};
use std::panic::catch_unwind;
use std::path::{PathBuf, Path};
use std::ffi::OsStr;
use rayon::iter::{IntoParallelIterator, ParallelIterator};

use exr::prelude::*;
use exr::error::{Error, UnitResult};
use exr::image::read::specific_channels::pixel_vec::PixelVec;

fn exr_files() -> impl Iterator<Item=PathBuf> {
    walkdir::WalkDir::new("tests/images/valid").into_iter().map(std::result::Result::unwrap)
        .filter(|entry| entry.path().extension() == Some(OsStr::new("exr")))
        .map(walkdir::DirEntry::into_path)
}

/// read all images in a directory.
/// does not check any content, just checks whether a read error or panic happened.
fn check_files<T>(
    ignore: Vec<PathBuf>,
    operation: impl Sync + std::panic::RefUnwindSafe + Fn(&Path) -> exr::error::Result<T>
) {
    #[derive(Debug, Eq, PartialEq, Ord, PartialOrd)]
    enum Result { Ok, Skipped, Unsupported(String), Error(String) };

    let files: Vec<PathBuf> = exr_files().collect();
    let mut results: Vec<(PathBuf, Result)> = files.into_par_iter()
        .map(|file| {
            if ignore.contains(&file) {
                return (file, Result::Skipped);
            }

            let result = catch_unwind(||{
                let prev_hook = panic::take_hook();
                panic::set_hook(Box::new(|_| (/* do not println panics */)));
                let result = operation(&file);
                panic::set_hook(prev_hook);

                result
            });

            let result = match result {
                Ok(Ok(_)) => Result::Ok,
                Ok(Err(Error::NotSupported(message))) => Result::Unsupported(message.to_string()),

                Ok(Err(Error::Io(io))) => Result::Error(format!("IoError: {:?}", io)),
                Ok(Err(Error::Invalid(message))) => Result::Error(format!("Invalid: {:?}", message)),
                Ok(Err(Error::Aborted)) => panic!("a test produced `Error::Abort`"),

                Err(_) => Result::Error("Panic".to_owned()),
            };

            match &result {
                Result::Error(_) => println!("✗ Error when processing {:?}", file),
                _ => println!("✓ No error when processing {:?}", file)
            };

            (file, result)
        })
        .collect();

    results.sort_by(|(_, a), (_, b)| a.cmp(b));

    println!("{:#?}", results.iter().map(|(path, result)| {
        format!("{:?}: {}", result, path.to_str().unwrap())
    }).collect::<Vec<_>>());

    assert!(results.len() >= 100, "Not all files were tested!");

    if let Result::Error(_) = results.last().unwrap().1 {
        panic!("A file triggered a panic");
    }
}

#[test]
fn round_trip_all_files_full() {
    println!("checking full feature set");
    check_files(vec![], |path| {
        let read_image = read()
            .no_deep_data().all_resolution_levels().all_channels().all_layers().all_attributes()
            .non_parallel();

        let image = read_image.clone().from_file(path)?;

        let mut tmp_bytes = Vec::new();
        image.write().non_parallel().to_buffered(Cursor::new(&mut tmp_bytes))?;

        let image2 = read_image.from_buffered(Cursor::new(tmp_bytes))?;

        assert_eq!(image.contains_nan_pixels(), image2.contains_nan_pixels());
        if !image.contains_nan_pixels() { assert_eq!(image, image2); } // thanks, NaN

        Ok(())
    })
}

#[test]
fn round_trip_all_files_simple() {
    println!("checking full feature set but not resolution levels");
    check_files(vec![], |path| {
        let read_image = read()
            .no_deep_data().largest_resolution_level().all_channels().all_layers().all_attributes()
            .non_parallel();

        let image = read_image.clone().from_file(path)?;

        let mut tmp_bytes = Vec::new();
        image.write().non_parallel().to_buffered(&mut Cursor::new(&mut tmp_bytes))?;

        let image2 = read_image.from_buffered(Cursor::new(&tmp_bytes))?;

        assert_eq!(image.contains_nan_pixels(), image2.contains_nan_pixels());
        if !image.contains_nan_pixels() { assert_eq!(image, image2); } // thanks, NaN

        Ok(())
    })
}

#[test]
fn round_trip_all_files_rgba() {

    // these files are known to be invalid, because they do not contain any rgb channels
    let blacklist = vec![
        PathBuf::from("tests/images/valid/openexr/LuminanceChroma/Garden.exr"),
        PathBuf::from("tests/images/valid/openexr/MultiView/Fog.exr"),
        PathBuf::from("tests/images/valid/openexr/TestImages/GrayRampsDiagonal.exr"),
        PathBuf::from("tests/images/valid/openexr/TestImages/GrayRampsHorizontal.exr"),
        PathBuf::from("tests/images/valid/openexr/TestImages/WideFloatRange.exr"),
        PathBuf::from("tests/images/valid/openexr/IlmfmlmflmTest/v1.7.test.tiled.exr")
    ];

    println!("checking rgba feature set");
    check_files(blacklist, |path| {
        let image_reader = read()
            .no_deep_data()
            .largest_resolution_level() // TODO all levels
            .rgba_channels(
                read::specific_channels::pixel_vec::create_pixel_vec,
                read::specific_channels::pixel_vec::set_pixel_in_vec::<(f32, f32, f32, Option<f32>)>,
            )
            .first_valid_layer()
            .all_attributes()
            .non_parallel();

        let image = image_reader.clone().from_file(path)?;

        let mut tmp_bytes = Vec::new();

        image.write().non_parallel()
            .to_buffered(&mut Cursor::new(&mut tmp_bytes))?;

        let image2 = image_reader.from_buffered(Cursor::new(&tmp_bytes))?;

        assert_eq!(image.contains_nan_pixels(), image2.contains_nan_pixels());
        if !image.contains_nan_pixels() { assert_eq!(image, image2); } // thanks, NaN

        Ok(())
    })
}

// TODO compare rgba vs rgb images for color content, and rgb vs rgb(a?)


#[test]
fn round_trip_parallel_files() {
    check_files(vec![], |path| {

        // let image = Image::read_from_file(path, read_options::high())?;
        let image = read()
            .no_deep_data().all_resolution_levels().all_channels().all_layers().all_attributes()
            .from_file(path)?;


        let mut tmp_bytes = Vec::new();
        // image.write_to_buffered(&mut Cursor::new(&mut tmp_bytes), write_options::high())?;
        image.write().to_buffered(Cursor::new(&mut tmp_bytes))?;

        // let image2 = Image::read_from_buffered(&mut tmp_bytes.as_slice(), ReadOptions{ pedantic: true, .. read_options::high() })?;
        let image2 = read()
            .no_deep_data().all_resolution_levels().all_channels().all_layers().all_attributes()
            .pedantic()
            .from_buffered(Cursor::new(tmp_bytes.as_slice()))?;

        assert_eq!(image.contains_nan_pixels(), image2.contains_nan_pixels());
        if !image.contains_nan_pixels() { assert_eq!(image, image2); } // thanks, NaN

        Ok(())
    })
}

#[test]
fn roundtrip_unusual_rgba() -> UnitResult {
    let image_reader = read()
        .no_deep_data()
        .largest_resolution_level() // TODO all levels
        .rgba_channels(
            read::specific_channels::pixel_vec::create_pixel_vec,
            read::specific_channels::pixel_vec::set_pixel_in_vec::<(f32, f32, f16, f32)>,
        )
        .first_valid_layer()
        .all_attributes()
        .non_parallel();

    let random_pixels: Vec<(f32, f32, f16, f32)> = vec![
        (0.1, 0.4, f16::from_f32(-5.0), 0.4),
        (0.3, 0.8, f16::from_f32(4.0), -0.4),
        (0.2, -0.6, f16::from_f32(2.0), -0.2),
        (0.8, 0.2, f16::from_f32(21.0), -0.4),
        (0.9, 0.0, f16::from_f32(64.0), 0.4),
    ];

    let size = Vec2(31, 7);
    let pixels = (0..size.area())
        .zip(random_pixels.into_iter().cycle())
        .map(|(_index, color)| color).collect::<Vec<_>>();

    let pixels = PixelVec { resolution: size, pixels: pixels };

    let image = Image::with_single_layer(size, SpecificChannels::named(
        ("R", "G", "B", "A"),
        pixels.clone()
    ));

    let mut tmp_bytes = Vec::new();
    image.write().non_parallel().to_buffered(&mut Cursor::new(&mut tmp_bytes))?;

    let image2 = image_reader.from_buffered(Cursor::new(&tmp_bytes))?;

    // custom compare function: considers nan equal to nan
    assert_eq!(image.layer_data.size, size, "test is buggy");
    let pixels1 = &image.layer_data.channel_data.storage;
    let pixels2 = &image2.layer_data.channel_data.storage;

    assert_eq!(pixels1, pixels2);

    Ok(())
}