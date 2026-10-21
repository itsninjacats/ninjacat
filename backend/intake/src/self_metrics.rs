//! Counters the intake keeps about itself, stored as ordinary metrics (`ninjacat.intake.<name>`)
//! so they can be graphed like any other.

use chrono::Utc;
use ninjapaws_storage::rows::Tags;
use ninjapaws_storage::rows::metrics::MetricPoint;

use crate::server::Ctx;

pub const IMAGES_SKIPPED: &str = "images.skipped";

pub fn count(ctx: &Ctx, host: &str, name: &str, value: f64, tags: Tags) {
    if value != 0.0 {
        ctx.write(vec![MetricPoint {
            host: host.into(),
            metric_type: "COUNT".into(),
            source_type: "ninjacat".into(),
            tags,
            ..MetricPoint::new(ctx.tenant(), Utc::now(), &format!("ninjacat.intake.{name}"), value)
        }]);
    }
}
