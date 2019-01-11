// Copyright 2017-2018 Kisio Digital and/or its affiliates.
//
// This program is free software: you can redistribute it and/or
// modify it under the terms of the GNU General Public License as
// published by the Free Software Foundation, either version 3 of the
// License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
// General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see
// <http://www.gnu.org/licenses/>.

//! See function apply_rules

use crate::collection::{CollectionWithId, Id};
use crate::model::Collections;
use crate::objects::{Codes, Geometry};
use crate::utils::{Report, ReportType};
use crate::Result;
use csv;
use failure::ResultExt;
use geo_types::Geometry as GeoGeometry;
use log::{info, warn};
use serde_derive::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use wkt::{self, conversion::try_into_geometry};

#[derive(Deserialize, Debug, Ord, PartialOrd, Eq, PartialEq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum ObjectType {
    Line,
    Route,
    StopPoint,
    StopArea,
}
impl ObjectType {
    pub fn as_str(&self) -> &'static str {
        match *self {
            ObjectType::Line => "line",
            ObjectType::Route => "route",
            ObjectType::StopPoint => "stop_point",
            ObjectType::StopArea => "stop_area",
        }
    }
}

#[derive(Deserialize, Debug, Ord, Eq, PartialOrd, PartialEq, Clone)]
struct ComplementaryCode {
    object_type: ObjectType,
    object_id: String,
    object_system: String,
    object_code: String,
}

#[derive(Deserialize, Debug, Ord, Eq, PartialOrd, PartialEq, Clone)]
struct PropertyRule {
    object_type: ObjectType,
    object_id: String,
    property_name: String,
    property_old_value: Option<String>,
    property_value: String,
}

fn read_complementary_code_rules_files<P: AsRef<Path>>(
    rule_files: Vec<P>,
    report: &mut Report,
) -> Result<Vec<ComplementaryCode>> {
    info!("Reading complementary code rules.");
    let mut codes = BTreeSet::new();
    for rule_path in rule_files {
        let path = rule_path.as_ref();
        let mut rdr = csv::ReaderBuilder::new()
            .trim(csv::Trim::All)
            .from_path(&path)
            .with_context(ctx_from_path!(path))?;
        for c in rdr.deserialize() {
            let c: ComplementaryCode = match c {
                Ok(val) => val,
                Err(e) => {
                    report.add_warning(
                        format!("Error reading {:?}: {}", path.file_name().unwrap(), e),
                        ReportType::InvalidFile,
                    );
                    continue;
                }
            };
            codes.insert(c);
        }
    }
    Ok(codes.into_iter().collect())
}

fn read_property_rules_files<P: AsRef<Path>>(
    rule_files: Vec<P>,
    report: &mut Report,
) -> Result<Vec<PropertyRule>> {
    info!("Reading property rules.");
    let mut properties: BTreeMap<(ObjectType, String, String), BTreeSet<PropertyRule>> =
        BTreeMap::default();
    for rule_path in rule_files {
        let path = rule_path.as_ref();
        let mut rdr = csv::ReaderBuilder::new()
            .trim(csv::Trim::All)
            .from_path(&path)
            .with_context(ctx_from_path!(path))?;
        for p in rdr.deserialize() {
            let p: PropertyRule = match p {
                Ok(val) => val,
                Err(e) => {
                    report.add_warning(
                        format!("Error reading {:?}: {}", path.file_name().unwrap(), e),
                        ReportType::InvalidFile,
                    );
                    continue;
                }
            };

            properties
                .entry((p.object_type, p.object_id.clone(), p.property_name.clone()))
                .or_insert_with(BTreeSet::new)
                .insert(p);
        }
    }

    let properties = properties
        .into_iter()
        .filter(|((object_type, object_id, property_name), property)| {
            match object_type {
                ObjectType::Route
                    if ![
                        "route_name",
                        "direction_type",
                        "route_geometry",
                        "destination_id",
                    ]
                    .contains(&property_name.as_ref()) =>
                {
                    report.add_warning(
                        format!(
                            "object_type={}, object_id={}: unknown property_name {} defined",
                            object_type.as_str(), object_id, property_name,
                        ),
                        ReportType::UnknownPropertyName,
                    );
                    return false;
                }
                ObjectType::Line | ObjectType::StopPoint | ObjectType::StopArea => {
                    warn!(
                        "Changing properties for {:?} is not yet possible.",
                        object_type.as_str()
                    );
                    return false;
                }
                _ => {}
            }

            if property.len() > 1 {
                {
                    report.add_warning(
                        format!(
                            "object_type={}, object_id={}: multiple values specified for the property {}",
                            object_type.as_str(), object_id, property_name
                        ),
                        ReportType::MultipleValue,
                    );
                }
                return false;
            }
            true
        })
        .flat_map(|(_, p)| p)
        .collect();

    Ok(properties)
}

fn insert_code<T>(
    collection: &mut CollectionWithId<T>,
    code: ComplementaryCode,
    report: &mut Report,
) where
    T: Codes + Id<T>,
{
    let idx = match collection.get_idx(&code.object_id) {
        Some(idx) => idx,
        None => {
            report.add_warning(
                format!(
                    "Error inserting code: object_codes.txt: object={},  object_id={} not found",
                    code.object_type.as_str(),
                    code.object_id
                ),
                ReportType::ObjectNotFound,
            );
            return;
        }
    };

    collection
        .index_mut(idx)
        .codes_mut()
        .insert((code.object_system, code.object_code));
}

fn update_prop<T: Clone + From<String> + Into<Option<String>>>(
    p: &PropertyRule,
    field: &mut T,
    report: &mut Report,
) {
    let any_prop = Some("*".to_string());
    if p.property_old_value == any_prop || p.property_old_value == field.clone().into() {
        *field = T::from(p.property_value.clone());
    } else {
        report.add_warning(
            format!(
                "object_type={}, object_id={}, property_name={}: property_old_value does not match the value found in the data",
                p.object_type.as_str(),
                p.object_id,
                p.property_name
            ),
            ReportType::OldPropertyValueDoesNotMatch,
        );
    }
}

fn wkt_to_geo(wkt: &str, report: &mut Report, p: &PropertyRule) -> Option<GeoGeometry<f64>> {
    if let Ok(wkt) = wkt::Wkt::from_str(wkt) {
        if let Ok(geo) = try_into_geometry(&wkt.items[0]) {
            Some(geo)
        } else {
            warn!("impossible to convert empty point");
            None
        }
    } else {
        report.add_warning(
            format!(
                "object_type={}, object_id={}: invalid geometry",
                p.object_type.as_str(),
                p.object_id,
            ),
            ReportType::GeometryNotValid,
        );
        None
    }
}

fn get_geometry_id(
    wkt: &str,
    collection: &mut CollectionWithId<Geometry>,
    p: &PropertyRule,
    report: &mut Report,
) -> Option<String> {
    if let Some(geo) = wkt_to_geo(wkt, report, p) {
        let id = p.object_type.as_str().to_owned() + ":" + &p.object_id;
        collection
            .get_mut(&id)
            .map(|mut g| {
                g.geometry = geo.clone();
            })
            .unwrap_or_else(|| {
                collection
                    .push(Geometry {
                        id: id.clone(),
                        geometry: geo,
                    })
                    .unwrap();
            });
        return Some(id);
    }

    None
}

fn update_geometry(
    p: &mut PropertyRule,
    geo_id: &mut Option<String>,
    report: &mut Report,
    geometries: &mut CollectionWithId<Geometry>,
) {
    match (p.property_old_value.as_ref(), geo_id.as_ref()) {
        (None, None) => {}
        (Some(pov), Some(geo_id)) => {
            if *pov == "*" {
                return;
            }
            let pov_geo = match wkt_to_geo(&pov, report, &p) {
                Some(pov_geo) => pov_geo,
                None => return,
            };
            let route_geo = match geometries.get(geo_id) {
                Some(geo) => &geo.geometry,
                None => {
                    // this should not happen
                    report.add_warning(
                        format!(
                            "object_type={}, object_id={}: geometry {} not found",
                            p.object_type.as_str(),
                            p.object_id,
                            geo_id
                        ),
                        ReportType::ObjectNotFound,
                    );
                    return;
                }
            };

            p.property_old_value = if &pov_geo != route_geo {
                None
            } else {
                Some(geo_id.to_string())
            }
        }
        (_, _) => {
            p.property_old_value = None;
        }
    }

    if let Some(id) = get_geometry_id(&p.property_value, geometries, &p, report) {
        p.property_value = id;
        update_prop(&p, geo_id, report);
    }
}

/// Applying rules
///
/// `complementary_code_rules_files` Csv files containing codes to add for certain objects
pub fn apply_rules(
    collections: &mut Collections,
    complementary_code_rules_files: Vec<PathBuf>,
    property_rules_files: Vec<PathBuf>,
    report_path: PathBuf,
) -> Result<()> {
    info!("Applying rules...");
    let mut report = Report::default();
    let codes = read_complementary_code_rules_files(complementary_code_rules_files, &mut report)?;
    for code in codes {
        match code.object_type {
            ObjectType::Line => insert_code(&mut collections.lines, code, &mut report),
            ObjectType::Route => insert_code(&mut collections.routes, code, &mut report),
            ObjectType::StopPoint => insert_code(&mut collections.stop_points, code, &mut report),
            ObjectType::StopArea => insert_code(&mut collections.stop_areas, code, &mut report),
        }
    }

    let properties = read_property_rules_files(property_rules_files, &mut report)?;
    for mut p in properties {
        match p.object_type {
            ObjectType::Route => {
                if let Some(mut route) = collections.routes.get_mut(&p.object_id) {
                    match p.property_name.as_str() {
                        "route_name" => update_prop(&p, &mut route.name, &mut report),
                        "direction_type" => update_prop(&p, &mut route.direction_type, &mut report),
                        "destination_id" => update_prop(&p, &mut route.destination_id, &mut report),
                        "route_geometry" => update_geometry(
                            &mut p,
                            &mut route.geometry_id,
                            &mut report,
                            &mut collections.geometries,
                        ),
                        _ => {}
                    }
                } else {
                    report.add_warning(
                        format!(
                            "{} {} not found in the data",
                            p.object_type.as_str(),
                            p.object_id
                        ),
                        ReportType::ObjectNotFound,
                    );
                }
            }
            _ => info!("not covered"),
        }
    }

    let serialized_report = serde_json::to_string_pretty(&report)?;
    fs::write(report_path, serialized_report)?;
    Ok(())
}
