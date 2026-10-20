use ninjapaws_anomaly::{Anomalies, Direction, Model, Seasonality, anomalies};

const MINUTE: i64 = 60_000;

/// Four hours of 20 s points with an hourly wave, and one spike in the last half hour.
fn series(spike_at: i64) -> Vec<(i64, f64)> {
    (0..4 * 180)
        .map(|i| {
            let t = 1_760_000_400_000 + i * 20_000;
            let wave = 10.0 * (2.0 * std::f64::consts::PI * (t % (60 * MINUTE)) as f64 / (60 * MINUTE) as f64).sin();
            let noise = ((i * 7919) % 13) as f64 / 13.0 - 0.5;
            (t, 50.0 + wave + noise + if t == spike_at { 25.0 } else { 0.0 })
        })
        .collect()
}

#[test]
fn every_algorithm_finds_the_spike_and_little_else() {
    let base = series(0);
    let from = base.last().unwrap().0 - 30 * MINUTE;
    let spike_at = from + 15 * MINUTE + 40_000;
    let points = series(spike_at);

    for algorithm in [Anomalies::Basic, Anomalies::Agile(Seasonality::Hourly), Anomalies::Robust(Seasonality::Hourly)] {
        let checked = anomalies(algorithm, &points, from, 3.0);
        assert!(checked.len() > 80, "{algorithm:?}: {} points with a band", checked.len());
        assert!(checked.iter().all(|p| p.time >= from));

        let anomalies: Vec<i64> = checked.iter().filter(|p| p.is_anomaly(Direction::Both)).map(|p| p.time).collect();
        assert!(anomalies.contains(&spike_at), "{algorithm:?}: spike not found");
        assert!(anomalies.len() <= 3, "{algorithm:?}: {} anomalies", anomalies.len());
        assert!(!checked.iter().any(|p| p.time == spike_at && p.is_anomaly(Direction::Below)));
    }
}

#[test]
fn a_seasonal_model_needs_three_seasons() {
    let points = series(0);
    let from = points[0].0 + 2 * 60 * MINUTE;
    assert!(Model::fit(Anomalies::Robust(Seasonality::Hourly), &points, from).is_none());
    assert!(anomalies(Anomalies::Agile(Seasonality::Hourly), &points, from, 2.0).is_empty());
}
