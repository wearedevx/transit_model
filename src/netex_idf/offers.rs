use crate::{model::Collections, Result};
use failure::format_err;
use log::{info, warn};
use minidom::Element;
use std::{fs::File, io::Read, path::Path};
use walkdir::WalkDir;

const CALENDAR_FILENAME: &str = "calendriers.xml";
pub fn read_offer_folder(offer_folder: &Path, _collections: &mut Collections) -> Result<()> {
    let calendar_path = offer_folder.join(CALENDAR_FILENAME);
    if calendar_path.exists() {
        let mut calendars_file = File::open(&calendar_path)?;
        let mut calendars_file_content = String::new();
        calendars_file.read_to_string(&mut calendars_file_content)?;
        let calendars: Element = calendars_file_content
            .parse()
            .map_err(|_| format_err!("Failed to open {:?}", calendar_path))?;
        info!("Reading {:?}", calendar_path);
        parse_calendars(&calendars)?;
    } else {
        warn!(
            "Offer {:?} ignored because it does not contain the '{}' file.",
            offer_folder, CALENDAR_FILENAME
        );
        return Ok(());
    }

    let common_path = offer_folder.join("commun.xml");
    if common_path.exists() {
        let mut common_file = File::open(&common_path)?;
        let mut common_file_content = String::new();
        common_file.read_to_string(&mut common_file_content)?;
        let common: Element = common_file_content
            .parse()
            .map_err(|_| format_err!("Failed to open {:?}", common_path))?;
        info!("Reading {:?}", common_path);
        parse_common(&common)?;
    }

    for offer_entry in WalkDir::new(offer_folder)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|dir_entry| dir_entry.file_type().is_file())
        .filter(|dir_entry| {
            dir_entry
                .path()
                .file_name()
                .and_then(|file_name| file_name.to_str())
                .map(|filename| filename.starts_with("offre_"))
                .unwrap_or_default()
        })
    {
        let offer_path = offer_entry.path();
        let mut offer_file = File::open(offer_path)?;
        let mut offer_file_content = String::new();
        offer_file.read_to_string(&mut offer_file_content)?;
        let offer: Element = offer_file_content
            .parse()
            .map_err(|_| format_err!("Failed to open {:?}", offer_path))?;
        info!("Reading {:?}", offer_entry.path());
        parse_offer(&offer)?;
    }
    Ok(())
}

fn parse_calendars(_calendars: &Element) -> Result<()> {
    // TODO: To implement
    Ok(())
}

fn parse_common(_common: &Element) -> Result<()> {
    // TODO: To implement
    Ok(())
}

fn parse_offer(_offer: &Element) -> Result<()> {
    // TODO: To implement
    Ok(())
}