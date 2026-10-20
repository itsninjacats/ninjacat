use ninjapaws_anomaly::{Forecast, LinearModel, Seasonality, forecast};

const MINUTE: i64 = 60_000;

#[test]
fn a_seasonal_forecast_carries_the_wave_on() {
    let wave = |t: i64| 50.0 + 10.0 * (2.0 * std::f64::consts::PI * (t % (60 * MINUTE)) as f64 / (60 * MINUTE) as f64).sin();
    let start = 1_760_000_400_000;
    let points: Vec<(i64, f64)> = (0..3 * 60).map(|i| start + i * MINUTE).map(|t| (t, wave(t))).collect();
    let last = points.last().unwrap().0;
    let future: Vec<i64> = (1..=30).map(|k| last + k * MINUTE).collect();

    let predicted = forecast(Forecast::Seasonal(Seasonality::Hourly), &points, &future, 2.0);
    assert_eq!(predicted.len(), 30);
    for p in &predicted {
        assert!((p.value - wave(p.time)).abs() < 0.5, "{p:?}");
        assert!(p.band.lower <= p.value && p.value <= p.band.upper);
    }
    let width = |i: usize| predicted[i].band.upper - predicted[i].band.lower;
    assert!(width(29) >= width(0), "the band widens with the horizon");
}

#[test]
fn a_linear_forecast_extends_the_trend() {
    let points: Vec<(i64, f64)> = (0..60).map(|i| (i * MINUTE, 10.0 + i as f64 + if i % 2 == 0 { 0.5 } else { -0.5 })).collect();
    let predicted = forecast(Forecast::Linear(LinearModel::Default), &points, &[70 * MINUTE], 1.0);
    assert!((predicted[0].value - 80.0).abs() < 1.0, "{predicted:?}");
    assert!(predicted[0].band.upper > predicted[0].band.lower);
}

#[test]
fn a_seasonal_forecast_needs_two_seasons() {
    let points: Vec<(i64, f64)> = (0..90).map(|i| (i * MINUTE, 1.0)).collect();
    assert!(forecast(Forecast::Seasonal(Seasonality::Hourly), &points, &[100 * MINUTE], 1.0).is_empty());
}
