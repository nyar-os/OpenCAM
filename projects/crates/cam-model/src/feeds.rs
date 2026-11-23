//! Cutting parameter recommendations.
//!
//! [`suggest`] turns a material and a tool diameter into a spindle speed, a
//! cutting feed and a plunge feed. The engine uses a fixed per-material surface
//! speed and chip-load table, so the same request always produces the same
//! numbers. It is an advisory calculation, not a machine simulation: travel
//! limits, rigidity and coolant stay the caller's responsibility.

use serde::{Deserialize, Serialize};

use crate::CamError;

/// Material names accepted by [`suggest`], in table order.
pub const MATERIALS: [&str; 6] = ["aluminium", "brass", "steel", "stainless", "plastic", "wood"];

/// Chip-load bands of one material: `(largest diameter, chip load per tooth)`.
type Bands = &'static [(f64, f64)];

/// Surface speed and chip-load bands of one material.
struct Material {
    /// Canonical name reported to the caller.
    name: &'static str,
    /// Cutting speed in metres per minute.
    surface_speed: f64,
    /// Chip load per tooth, by tool-diameter band.
    bands: Bands,
}

const ALUMINIUM: Bands = &[(3.0, 0.010), (6.0, 0.020), (12.0, 0.040), (f64::MAX, 0.060)];
const BRASS: Bands = &[(3.0, 0.008), (6.0, 0.016), (12.0, 0.030), (f64::MAX, 0.045)];
const STEEL: Bands = &[(3.0, 0.006), (6.0, 0.012), (12.0, 0.025), (f64::MAX, 0.035)];
const STAINLESS: Bands = &[(3.0, 0.004), (6.0, 0.008), (12.0, 0.015), (f64::MAX, 0.020)];
const PLASTIC: Bands = &[(3.0, 0.015), (6.0, 0.030), (12.0, 0.050), (f64::MAX, 0.070)];
const WOOD: Bands = &[(3.0, 0.020), (6.0, 0.040), (12.0, 0.070), (f64::MAX, 0.100)];

const TABLE: [Material; 6] = [
    Material { name: "aluminium", surface_speed: 300.0, bands: ALUMINIUM },
    Material { name: "brass", surface_speed: 200.0, bands: BRASS },
    Material { name: "steel", surface_speed: 120.0, bands: STEEL },
    Material { name: "stainless", surface_speed: 60.0, bands: STAINLESS },
    Material { name: "plastic", surface_speed: 400.0, bands: PLASTIC },
    Material { name: "wood", surface_speed: 500.0, bands: WOOD },
];

/// Default flute count when a request omits it.
pub const DEFAULT_FLUTES: u32 = 2;

/// Lowest spindle speed [`suggest`] reports, in revolutions per minute.
pub const MIN_SPINDLE: f64 = 100.0;

/// Highest spindle speed [`suggest`] reports, in revolutions per minute.
///
/// The ceiling keeps a very small tool from producing an unusable number; a
/// machine with a faster spindle can scale the recommendation itself.
pub const MAX_SPINDLE: f64 = 24000.0;

/// Lowest feed or plunge rate [`suggest`] reports, in millimetres per minute.
pub const MIN_FEED: f64 = 10.0;

/// Request of [`suggest`]: one material and one tool.
///
/// `flutes` is optional and defaults to [`DEFAULT_FLUTES`]; the material name is
/// case-sensitive. `"aluminum"` is accepted as an alias of `"aluminium"` and is
/// reported under the canonical name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedsRequest {
    /// Material name; one of [`MATERIALS`].
    pub material: String,
    /// Tool diameter in millimetres.
    pub diameter: f64,
    /// Number of flutes; `None` selects [`DEFAULT_FLUTES`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flutes: Option<u32>,
}

/// Recommended spindle and feeds of one tool in one material.
///
/// `spindle` is rounded to the nearest 100 rpm and clamped to
/// [`MIN_SPINDLE`] … [`MAX_SPINDLE`]; `feed` and `plungeFeed` are rounded to the
/// nearest 10 mm/min and never fall below [`MIN_FEED`]. `surfaceSpeed` is the
/// table cutting speed in metres per minute and `chipLoad` the selected chip
/// load per tooth in millimetres. The plunge feed is 30 % of the cutting feed, a
/// common entry rule.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedsReport {
    /// Report format identifier, always `open-cam-feeds`.
    pub format: String,
    /// Report version.
    pub version: u32,
    /// Unit system; always `mm`.
    pub units: String,
    /// Canonical material name that was looked up.
    pub material: String,
    /// Tool diameter the calculation used, in millimetres.
    pub diameter: f64,
    /// Flute count the calculation used.
    pub flutes: u32,
    /// Table cutting speed of the material, in metres per minute.
    pub surface_speed: f64,
    /// Selected chip load per tooth, in millimetres.
    pub chip_load: f64,
    /// Recommended spindle speed, in revolutions per minute.
    pub spindle: f64,
    /// Recommended cutting feed, in millimetres per minute.
    pub feed: f64,
    /// Recommended plunge feed, in millimetres per minute.
    pub plunge_feed: f64,
}

/// Suggests spindle and feed rates for one material and tool diameter.
///
/// The calculation is `spindle = 1000 × surface speed / (π × diameter)`,
/// `feed = spindle × chip load × flutes` and `plunge = 30 % × feed`, each
/// rounded as documented on [`FeedsReport`] and clamped to
/// [`MIN_SPINDLE`] … [`MAX_SPINDLE`] and to at least [`MIN_FEED`]. A
/// non-positive diameter, a zero flute count or an unknown material reports
/// `feeds/invalid` or `feeds/unknown-material`.
pub fn suggest(request: &FeedsRequest) -> Result<FeedsReport, CamError> {
    if !request.diameter.is_finite() || request.diameter <= 0.0 {
        return Err(CamError::new("feeds/invalid", "the tool diameter must be finite and greater than zero"));
    }
    let flutes = request.flutes.unwrap_or(DEFAULT_FLUTES);
    if flutes == 0 {
        return Err(CamError::new("feeds/invalid", "the flute count must be at least one"));
    }
    let material = lookup(&request.material).ok_or_else(|| {
        CamError::new(
            "feeds/unknown-material",
            format!(
                "material `{}` is unknown; only {} is supported",
                request.material,
                MATERIALS.iter().map(|name| format!("`{name}`")).collect::<Vec<_>>().join(", ")
            ),
        )
    })?;
    let chip_load = chip_load(material.bands, request.diameter);
    let spindle = round_to(1000.0 * material.surface_speed / (std::f64::consts::PI * request.diameter), 100.0)
        .clamp(MIN_SPINDLE, MAX_SPINDLE);
    let feed = round_to(spindle * chip_load * f64::from(flutes), 10.0).max(MIN_FEED);
    let plunge_feed = round_to(0.3 * feed, 10.0).max(MIN_FEED);
    Ok(FeedsReport {
        format: "open-cam-feeds".to_string(),
        version: 1,
        units: "mm".to_string(),
        material: material.name.to_string(),
        diameter: request.diameter,
        flutes,
        surface_speed: material.surface_speed,
        chip_load,
        spindle,
        feed,
        plunge_feed,
    })
}

/// Looks up one material, accepting `aluminum` as an alias of `aluminium`.
fn lookup(name: &str) -> Option<&'static Material> {
    let wanted = if name == "aluminum" { "aluminium" } else { name };
    TABLE.iter().find(|material| material.name == wanted)
}

/// Chip load of the first diameter band that covers `diameter`.
fn chip_load(bands: Bands, diameter: f64) -> f64 {
    bands.iter().find(|(largest, _)| diameter <= *largest).map_or_else(|| bands[bands.len() - 1].1, |(_, load)| *load)
}

/// Rounds `value` to the nearest multiple of `step`, normalising `-0`.
fn round_to(value: f64, step: f64) -> f64 {
    let rounded = (value / step).round() * step;
    if rounded.abs() < 1e-9 { 0.0 } else { rounded }
}
