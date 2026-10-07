//! `fc day-start-hour`, `fc backup-settings`, `fc device-id` and `fc rebuild-schedule`: the
//! settings a collection keeps and the repairs a developer may want, tried from a terminal.

use super::{Failure, open, plural};

fn number<T: std::str::FromStr>(name: &str, text: &str) -> Result<T, Failure> {
    text.parse()
        .map_err(|_| Failure::Usage(format!("\"{text}\" is not a valid value for {name}.")))
}

pub fn day_start_hour(file: &str, hour: Option<&String>) -> Result<String, Failure> {
    let collection = open(file)?;
    let text = match hour {
        Some(hour) => {
            let hour: u8 = number("the hour", hour)?;
            collection.set_day_start_hour(hour)?;
            format!(
                "The study day now starts at {hour}:00 local time. Cards keep the due days they have"
            )
        }
        None => format!(
            "The study day starts at {}:00 local time",
            collection.day_start_hour()?
        ),
    };
    collection.close()?;
    Ok(text)
}

pub fn backup_settings(file: &str, options: &[String]) -> Result<String, Failure> {
    let collection = open(file)?;
    let current = collection.backup_settings()?;
    let (mut interval, mut keep) = (current.interval_hours, current.keep);
    let mut changed = false;
    let mut options = options.iter();
    while let Some(option) = options.next() {
        let value = options
            .next()
            .ok_or_else(|| Failure::Usage(format!("{option} needs a value.")))?;
        match option.as_str() {
            "--interval" => interval = number("--interval", value)?,
            "--keep" => keep = number("--keep", value)?,
            other => return Err(Failure::Usage(format!("Unknown option \"{other}\"."))),
        }
        changed = true;
    }
    if changed {
        collection.set_backup_settings(interval, keep)?;
    }
    let settings = collection.backup_settings()?;
    collection.close()?;
    let mut text = format!(
        "Automatic backup: {}, keeping {}",
        if settings.interval_hours == 0 {
            "off".to_owned()
        } else {
            format!("every {}", plural(settings.interval_hours as usize, "hour"))
        },
        settings.keep
    );
    if let Some(error) = settings.last_error {
        text.push_str(&format!("\nThe last automatic backup failed: {error}"));
    }
    Ok(text)
}

pub fn device_id(file: &str, regenerate: bool) -> Result<String, Failure> {
    let collection = open(file)?;
    let text = if regenerate {
        format!("New device ID: {}", collection.regenerate_device_id()?)
    } else {
        format!("Device ID: {}", collection.device_id()?)
    };
    collection.close()?;
    Ok(text)
}

pub fn rebuild_schedule(file: &str) -> Result<String, Failure> {
    let collection = open(file)?;
    collection.rebuild_schedule()?;
    collection.close()?;
    Ok("Rebuilt the schedule of every card from its answers".to_owned())
}
