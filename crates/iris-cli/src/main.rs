//! iris-cli: headless RAW rendering using the same engine as the desktop app.
//!
//!   iris-cli INPUT OUTPUT [--quality N] [--long-edge N] [--16bit] [--edits FILE] [--set NAME=VALUE ...]
//!   iris-cli --info INPUT
//!
//! Edits come from the photo's .iris.json sidecar if it has one (or from --edits FILE);
//! --set then adjusts individual settings, e.g. --set exposure=0.7 --set temperature=5200.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use iris_core::edit_state::find_adjustment_field;
use iris_core::{BasicAdjustments, EditState};
use iris_export::{ExportFormat, ExportSettings, export_image};
use iris_persist::{read_sidecar_file, sidecar_path_for};
use iris_raw::{DecodeQuality, decode};

const USAGE: &str = "\
Usage: iris-cli INPUT OUTPUT [--quality N] [--long-edge N] [--16bit] [--edits FILE]
                    [--set NAME=VALUE ...]
       iris-cli --info INPUT
OUTPUT format is chosen from its extension: .jpg, .png or .tif
Settings: exposure contrast highlights shadows whites blacks
          temperature tint vibrance saturation";

/// A usage error: print the usage text and exit with status 2.
struct Usage;

fn apply_setting(a: &mut BasicAdjustments, assignment: &str) -> Option<()> {
    let (name, value) = assignment.split_once('=')?;
    let field = find_adjustment_field(name)?;
    *(field.value)(a) = value.trim().parse().ok()?;
    *a = a.sanitized();
    Some(())
}

fn info(input: &Path) -> Result<()> {
    let m = decode(input, DecodeQuality::Preview, &|| false)?.metadata;
    println!("Camera:      {} {}", m.make, m.model);
    println!("Lens:        {}", m.lens);
    println!("ISO:         {:.0}", m.iso);
    println!("Shutter:     {} s", m.shutter_seconds);
    println!("Aperture:    f/{:.1}", m.aperture);
    println!("Focal:       {:.0} mm", m.focal_length_mm);
    println!("Orientation: {}", m.orientation);
    println!("Size:        {} x {}", m.width, m.height);
    println!("As shot WB:  {:.0} K, tint {:+.0}", m.as_shot.temperature, m.as_shot.tint);
    Ok(())
}

struct Job {
    input: PathBuf,
    output: PathBuf,
    settings: ExportSettings,
    edits_file: Option<PathBuf>,
    assignments: Vec<String>,
}

fn parse(args: &[String]) -> Result<Job, Usage> {
    let [input, output, rest @ ..] = args else { return Err(Usage) };
    let output = PathBuf::from(output);
    let format = ExportFormat::from_path(&output).ok_or(Usage)?;
    let mut job = Job {
        input: input.into(),
        output,
        settings: ExportSettings { format, ..Default::default() },
        edits_file: None,
        assignments: Vec::new(),
    };
    let mut args = rest.iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(Usage);
        match arg.as_str() {
            "--quality" => job.settings.jpeg_quality = value()?.parse().map_err(|_| Usage)?,
            "--long-edge" => job.settings.long_edge = value()?.parse().map_err(|_| Usage)?,
            "--16bit" => job.settings.bits_per_channel = 16,
            "--edits" => job.edits_file = Some(value()?.into()),
            "--set" => job.assignments.push(value()?.clone()),
            _ => return Err(Usage),
        }
    }
    Ok(job)
}

fn run(job: &Job) -> Result<()> {
    let start = Instant::now();
    let decoded = decode(&job.input, DecodeQuality::Full, &|| false)?;
    println!("Decoded {} x {} in {:.2} s", decoded.image.width, decoded.image.height, start.elapsed().as_secs_f64());

    let as_shot = decoded.metadata.as_shot;
    let mut edits = EditState::new(as_shot);
    let sidecar = job.edits_file.clone().unwrap_or_else(|| sidecar_path_for(&job.input));
    if let Some(saved) =
        read_sidecar_file(&sidecar, &edits).with_context(|| format!("Cannot read {}", sidecar.display()))?
    {
        edits = saved;
        println!("Using edits from {}", sidecar.display());
    }
    for assignment in &job.assignments {
        if apply_setting(&mut edits.basic, assignment).is_none() {
            bail!("invalid setting {assignment:?}");
        }
    }

    let start = Instant::now();
    export_image(&decoded.image, &as_shot, &edits, &job.settings, &job.output)?;
    println!("Exported {} in {:.2} s", job.output.display(), start.elapsed().as_secs_f64());
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [flag, input] if flag == "--info" => info(Path::new(input)),
        _ => match parse(&args) {
            Ok(job) => run(&job),
            Err(Usage) => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        },
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("iris-cli: {e:#}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn parses_arguments() {
        let job =
            parse(&args("in.ARW out.png --long-edge 1200 --16bit --set exposure=0.5 --edits e.json")).ok().unwrap();
        assert_eq!(job.settings.format, ExportFormat::Png);
        assert_eq!(job.settings.long_edge, 1200);
        assert_eq!(job.settings.bits_per_channel, 16);
        assert_eq!(job.assignments, ["exposure=0.5"]);
        assert_eq!(job.edits_file, Some(PathBuf::from("e.json")));

        assert!(parse(&args("in.ARW out.webp")).is_err());
        assert!(parse(&args("in.ARW out.jpg --quality")).is_err());
        assert!(parse(&args("in.ARW")).is_err());
    }

    #[test]
    fn applies_settings() {
        let mut a = BasicAdjustments::default();
        assert!(apply_setting(&mut a, "exposure=9").is_some());
        assert_eq!(a.exposure, 5.0); // clamped
        assert!(apply_setting(&mut a, "temperature=5200").is_some());
        assert_eq!(a.white_balance.temperature, 5200.0);
        assert!(apply_setting(&mut a, "sharpness=3").is_none());
        assert!(apply_setting(&mut a, "contrast").is_none());
    }
}
