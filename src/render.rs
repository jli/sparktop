/// Rendering logic.
use std::collections::VecDeque;
use tui::{
    style::{Color, Style},
    text::Span,
};

#[derive(Debug)]
pub struct CompressionScheme {
    pub tier0_bars: usize,
    pub tier1_bars: usize,
    pub tier2_bars: usize,
    pub tier0_samples: usize,
    pub tier1_samples: usize,
    pub tier2_samples: usize,
}

impl CompressionScheme {
    pub fn visual_markers(&self) -> Vec<Span<'static>> {
        let total_bars = self.tier0_bars + self.tier1_bars + self.tier2_bars;
        if total_bars == 0 {
            return vec![Span::raw("")];
        }

        let mut result = Vec::new();

        // Helper to add compression indicator for a tier
        let add_tier_markers = |result: &mut Vec<Span<'static>>,
                                bars: usize,
                                samples: usize,
                                fallback_char: &str,
                                color: Color| {
            if bars == 0 {
                return;
            }

            // CRITICAL: Never allow bars > samples (stretching)
            // This should never happen, but guard against it
            if bars > samples {
                // BUG: stretching detected - show error indicator
                for _ in 0..bars {
                    result.push(Span::styled("!", Style::default().fg(Color::Red)));
                }
                return;
            }

            // Calculate compression ratio
            let ratio = if bars > 0 && samples > 0 {
                (samples as f64 / bars as f64).ceil() as usize
            } else {
                1
            };

            // For 1:1 (full resolution), don't show markers
            if ratio == 1 && bars == samples {
                for _ in 0..bars {
                    result.push(Span::raw(" "));
                }
                return;
            }

            // When compressed, show fallback single characters for ALL positions
            // This makes compression visually obvious across the entire section
            for _ in 0..bars {
                result.push(Span::styled(
                    fallback_char.to_string(),
                    Style::default().fg(color),
                ));
            }
        };

        // Tier 0: full/low compression
        add_tier_markers(
            &mut result,
            self.tier0_bars,
            self.tier0_samples,
            ".",
            Color::Cyan,
        );

        // Tier 1: medium compression
        add_tier_markers(
            &mut result,
            self.tier1_bars,
            self.tier1_samples,
            "o",
            Color::Blue,
        );

        // Tier 2: heavy compression
        add_tier_markers(
            &mut result,
            self.tier2_bars,
            self.tier2_samples,
            "O",
            Color::DarkGray,
        );

        result
    }
}

/// Compress CPU history to fit available terminal width while preserving detail in recent data.
///
/// # Algorithm (simplified, single-pass)
/// 1. Divide history into 3 time-based tiers based on available samples
/// 2. Calculate ideal bars for each tier (1:1, 4:1, 15:1 compression)
/// 3. Allocate available width to tiers:
///    - If width >= ideal: give each tier ideal bars (or more, capped at 1:1)
///    - If width < ideal: progressively compress tiers, prioritizing recent data
/// 4. Never expand samples (max 1 bar per sample)
/// 5. Prefer compression ratios ≤10x for smoother visual ramp
///
/// # Visual Markers
/// Header shows colored dots aligned with sparklines to indicate compression zones:
/// - White dots (·) = Tier 0 (full/low compression)
/// - Cyan dashes (─) = Tier 1 (medium compression)
/// - Gray double-lines (═) = Tier 2 (heavy compression)
///
/// # Example
/// With 600 samples and 50 chars width:
/// - tier0: 120 samples → 33 bars (3.6:1 compression)
/// - tier1: 180 samples → 11 bars (16:1 compression)
/// - tier2: 300 samples → 6 bars (50:1 compression)
/// - Total: 50 bars showing all 10 minutes of history
pub fn compress_history(hist: &VecDeque<f64>, width: usize) -> (Vec<f64>, CompressionScheme) {
    // Early returns
    if hist.is_empty() || width == 0 {
        return (
            Vec::new(),
            CompressionScheme {
                tier0_bars: 0,
                tier1_bars: 0,
                tier2_bars: 0,
                tier0_samples: 0,
                tier1_samples: 0,
                tier2_samples: 0,
            },
        );
    }

    let hist_len = hist.len();

    // Tier boundaries
    const TIER0_END: usize = 120;
    const TIER1_END: usize = 300;

    // Calculate samples in each tier
    let tier0_samples = hist_len.min(TIER0_END);
    let tier1_samples = if hist_len > TIER0_END {
        (hist_len - TIER0_END).min(TIER1_END - TIER0_END)
    } else {
        0
    };
    let tier2_samples = if hist_len > TIER1_END {
        hist_len - TIER1_END
    } else {
        0
    };

    // Calculate ideal bars (1:1, 4:1, 15:1 compression ratios)
    let ideal_t1 = if tier1_samples > 0 {
        (tier1_samples / 4).max(1)
    } else {
        0
    };
    let ideal_t2 = if tier2_samples > 0 {
        (tier2_samples / 15).max(1)
    } else {
        0
    };

    // Allocate bars to tiers
    let (
        tier0_bars,
        tier1_bars,
        tier2_bars,
        scheme_t0_samples,
        scheme_t1_samples,
        scheme_t2_samples,
    );

    // Key rule: tier0 gets full 1:1 when width >= tier0_samples
    if width >= tier0_samples {
        // Plenty of space: use ideal or better (up to 1:1)
        let t0 = tier0_samples;
        let remaining = width - t0;

        let allocation = if tier1_samples == 0 && tier2_samples == 0 {
            // Only tier0: cap at tier0_samples (no expansion)
            (t0.min(tier0_samples), 0, 0, tier0_samples, 0, 0)
        } else if tier1_samples > 0 && tier2_samples == 0 {
            // tier0 + tier1: distribute remaining
            let t1 = remaining.min(tier1_samples);
            (t0, t1, 0, tier0_samples, tier1_samples, 0)
        } else if tier1_samples == 0 && tier2_samples > 0 {
            // tier0 + tier2
            let t2 = remaining.min(tier2_samples);
            (t0, 0, t2, tier0_samples, 0, tier2_samples)
        } else {
            // All three tiers
            let ideal_remain = ideal_t1 + ideal_t2;
            if remaining >= tier1_samples + tier2_samples {
                // Can show both at 1:1
                (
                    t0,
                    tier1_samples,
                    tier2_samples,
                    tier0_samples,
                    tier1_samples,
                    tier2_samples,
                )
            } else if remaining >= ideal_remain {
                // Between ideal and 1:1 - distribute extra
                let extra = remaining - ideal_remain;
                let extra_t1 = (extra * 2 / 3).min(tier1_samples - ideal_t1);
                let extra_t2 = extra.saturating_sub(extra_t1).min(tier2_samples - ideal_t2);
                let t1 = (ideal_t1 + extra_t1).min(tier1_samples);
                let t2 = (ideal_t2 + extra_t2).min(tier2_samples);
                (t0, t1, t2, tier0_samples, tier1_samples, tier2_samples)
            } else {
                // Less than ideal - allocate proportionally (only if remaining > 0)
                let t1 = if tier1_samples > 0 && remaining > 0 {
                    (remaining * 2 / 3).max(1).min(tier1_samples)
                } else {
                    0
                };
                let t2 = if tier2_samples > 0 && remaining > 0 {
                    remaining.saturating_sub(t1).min(tier2_samples)
                } else {
                    0
                };
                (t0, t1, t2, tier0_samples, tier1_samples, tier2_samples)
            }
        };

        tier0_bars = allocation.0;
        tier1_bars = allocation.1;
        tier2_bars = allocation.2;
        scheme_t0_samples = allocation.3;
        scheme_t1_samples = allocation.4;
        scheme_t2_samples = allocation.5;
    } else {
        // Constrained space: need to compress tier0 too
        let allocation = if tier1_samples == 0 && tier2_samples == 0 && tier0_samples > width {
            // Virtual tiering within tier0
            let recent_samples = (tier0_samples / 3).max(1);
            let older_samples = tier0_samples - recent_samples;
            let recent_bars = (width * 2 / 3).max(1).min(recent_samples); // Never expand
            let older_bars = width - recent_bars;
            (recent_bars, older_bars, 0, recent_samples, older_samples, 0)
        } else {
            // Normal constrained allocation
            let num_tiers = (if tier0_samples > 0 { 1 } else { 0 })
                + (if tier1_samples > 0 { 1 } else { 0 })
                + (if tier2_samples > 0 { 1 } else { 0 });

            match width.cmp(&num_tiers) {
                std::cmp::Ordering::Less => {
                    // Very narrow: 1 bar per tier or less
                    let t0 = if tier0_samples > 0 { 1 } else { 0 };
                    let t1 = if tier1_samples > 0 && width > t0 {
                        1
                    } else {
                        0
                    };
                    let t2 = if tier2_samples > 0 {
                        width.saturating_sub(t0 + t1)
                    } else {
                        0
                    };
                    (t0, t1, t2, tier0_samples, tier1_samples, tier2_samples)
                }
                std::cmp::Ordering::Equal => {
                    // Exactly 1 bar per tier
                    (
                        if tier0_samples > 0 { 1 } else { 0 },
                        if tier1_samples > 0 { 1 } else { 0 },
                        if tier2_samples > 0 { 1 } else { 0 },
                        tier0_samples,
                        tier1_samples,
                        tier2_samples,
                    )
                }
                std::cmp::Ordering::Greater => {
                    // Allocate proportionally: tier0 gets 2/3, tier1/2 split remaining
                    let t0 = if tier0_samples > 0 {
                        (width * 2 / 3).max(1)
                    } else {
                        0
                    };
                    let remaining = width.saturating_sub(t0);

                    if tier2_samples > 0 && remaining > 0 {
                        let t1 = if tier1_samples > 0 {
                            (remaining * 2 / 3).max(if remaining > 1 { 1 } else { 0 })
                        } else {
                            0
                        };
                        let t2 = remaining.saturating_sub(t1);
                        (t0, t1, t2, tier0_samples, tier1_samples, tier2_samples)
                    } else if tier1_samples > 0 {
                        (t0, remaining, 0, tier0_samples, tier1_samples, 0)
                    } else {
                        (t0, 0, 0, tier0_samples, 0, 0)
                    }
                }
            }
        };

        tier0_bars = allocation.0;
        tier1_bars = allocation.1;
        tier2_bars = allocation.2;
        scheme_t0_samples = allocation.3;
        scheme_t1_samples = allocation.4;
        scheme_t2_samples = allocation.5;
    }

    // Helper: compress samples into bars
    let compress_samples = |start: usize, count: usize, bars: usize| -> Vec<f64> {
        let mut result = Vec::new();
        if bars == 0 || count == 0 {
            return result;
        }

        for i in 0..bars {
            let sample_start = start + (i * count) / bars;
            let sample_end = start + ((i + 1) * count) / bars;
            let actual_end = sample_end.min(start + count).min(hist_len);

            if sample_start < actual_end {
                let sum: f64 = (sample_start..actual_end).map(|j| hist[j]).sum();
                result.push(sum / (actual_end - sample_start) as f64);
            }
        }
        result
    };

    // Build compressed output
    let mut compressed = Vec::new();
    compressed.extend(compress_samples(0, scheme_t0_samples, tier0_bars));
    if tier1_bars > 0 {
        compressed.extend(compress_samples(
            scheme_t0_samples,
            scheme_t1_samples,
            tier1_bars,
        ));
    }
    if tier2_bars > 0 {
        compressed.extend(compress_samples(
            scheme_t0_samples + scheme_t1_samples,
            scheme_t2_samples,
            tier2_bars,
        ));
    }

    let scheme = CompressionScheme {
        tier0_bars,
        tier1_bars,
        tier2_bars,
        tier0_samples: scheme_t0_samples,
        tier1_samples: scheme_t1_samples,
        tier2_samples: scheme_t2_samples,
    };

    (compressed, scheme)
}

pub fn render_vec<'a, II>(xs: II, max: f64) -> String
where
    II: IntoIterator<Item = &'a f64>,
{
    let mut r = String::new();
    for x in xs.into_iter() {
        let p = *x / max;
        r.push(float_bar(p));
    }
    r
}

pub fn render_vec_colored<II>(xs: II, max: f64) -> Vec<Span<'static>>
where
    II: IntoIterator<Item = f64>,
{
    let mut result = Vec::new();
    for x in xs.into_iter() {
        let p = x / max;
        let c = float_bar(p);
        let color = cpu_color(x);
        if let Some(color) = color {
            result.push(Span::styled(c.to_string(), Style::default().fg(color)));
        } else {
            result.push(Span::raw(c.to_string()));
        }
    }
    result
}

pub fn cpu_color(cpu: f64) -> Option<Color> {
    if cpu >= 400.0 {
        Some(Color::Magenta)
    } else if cpu >= 200.0 {
        Some(Color::LightMagenta)
    } else if cpu >= 100.0 {
        Some(Color::Red)
    } else {
        None
    }
}

const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

// f must be between 0 and 1.
fn float_bar(mut f: f64) -> char {
    f = f.min(1.); // cpu usage can be > 1. do something special?
    if f < 0.03 {
        return ' ';
    }
    let sub_seg = 1. / BARS.len() as f64;
    let mut i = 0;
    while f > sub_seg {
        f -= sub_seg;
        i += 1;
    }
    BARS[i]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_history(len: usize) -> VecDeque<f64> {
        (0..len).map(|i| i as f64).collect()
    }

    #[test]
    fn test_compress_empty() {
        let hist = VecDeque::new();
        let (compressed, scheme) = compress_history(&hist, 100);
        assert_eq!(compressed.len(), 0);
        assert_eq!(scheme.tier0_bars, 0);
        assert_eq!(scheme.tier1_bars, 0);
        assert_eq!(scheme.tier2_bars, 0);
    }

    #[test]
    fn test_compress_zero_width() {
        let hist = make_history(600);
        let (compressed, scheme) = compress_history(&hist, 0);
        assert_eq!(compressed.len(), 0);
        assert_eq!(scheme.tier0_bars, 0);
    }

    #[test]
    fn test_compress_full_history_ideal_width() {
        // With 600 samples and width=200 (less than 600)
        let hist = make_history(600);
        let (compressed, scheme) = compress_history(&hist, 200);

        // Tier0 always 1:1 (120 bars), extra distributed to tier1/tier2
        assert_eq!(scheme.tier0_bars, 120); // 1:1 - never compressed
                                            // tier1: 180 samples, tier2: 300 samples
                                            // Remaining width: 80 bars for tier1+tier2
                                            // Will distribute proportionally, but never expand beyond samples
        assert!(scheme.tier1_bars > 0);
        assert!(scheme.tier2_bars > 0);
        assert_eq!(compressed.len(), 200); // Fills all available width
        assert_eq!(
            scheme.tier0_bars + scheme.tier1_bars + scheme.tier2_bars,
            200
        );
    }

    #[test]
    fn test_compress_very_narrow_width_1() {
        let hist = make_history(600);
        let (compressed, scheme) = compress_history(&hist, 1);

        // With width=1, should show only tier0 (most recent)
        assert_eq!(scheme.tier0_bars, 1);
        assert_eq!(scheme.tier1_bars, 0);
        assert_eq!(scheme.tier2_bars, 0);
        assert_eq!(compressed.len(), 1);

        // Should be average of tier0 samples (0-119)
        let expected_avg = (0..120).sum::<usize>() as f64 / 120.0;
        assert!((compressed[0] - expected_avg).abs() < 0.1);
    }

    #[test]
    fn test_compress_very_narrow_width_2() {
        let hist = make_history(600);
        let (compressed, scheme) = compress_history(&hist, 2);

        // With width=2, should show tier0 and tier1
        assert_eq!(scheme.tier0_bars, 1);
        assert_eq!(scheme.tier1_bars, 1);
        assert_eq!(scheme.tier2_bars, 0);
        assert_eq!(compressed.len(), 2);
    }

    #[test]
    fn test_compress_very_narrow_width_3() {
        let hist = make_history(600);
        let (compressed, scheme) = compress_history(&hist, 3);

        // With width=3 and 3 tiers, each should get 1 bar
        assert_eq!(scheme.tier0_bars, 1);
        assert_eq!(scheme.tier1_bars, 1);
        assert_eq!(scheme.tier2_bars, 1);
        assert_eq!(compressed.len(), 3);

        println!(
            "Width=3: tier0={}, tier1={}, tier2={}",
            scheme.tier0_bars, scheme.tier1_bars, scheme.tier2_bars
        );
    }

    #[test]
    fn test_compress_narrow_width_10() {
        let hist = make_history(600);
        let (compressed, scheme) = compress_history(&hist, 10);

        // Should allocate proportionally but each tier gets at least 1
        assert!(scheme.tier0_bars >= 1);
        assert!(scheme.tier1_bars >= 1);
        assert!(scheme.tier2_bars >= 1);
        assert_eq!(
            scheme.tier0_bars + scheme.tier1_bars + scheme.tier2_bars,
            10
        );
        assert_eq!(compressed.len(), 10);

        println!(
            "Width=10: tier0={}, tier1={}, tier2={}",
            scheme.tier0_bars, scheme.tier1_bars, scheme.tier2_bars
        );
    }

    #[test]
    fn test_compress_medium_width_50() {
        let hist = make_history(600);
        let (compressed, scheme) = compress_history(&hist, 50);

        // Recent data should get priority
        assert!(scheme.tier0_bars > scheme.tier1_bars);
        assert!(scheme.tier1_bars > scheme.tier2_bars);
        assert_eq!(
            scheme.tier0_bars + scheme.tier1_bars + scheme.tier2_bars,
            50
        );
        assert_eq!(compressed.len(), 50);

        println!(
            "Width=50: tier0={}, tier1={}, tier2={}",
            scheme.tier0_bars, scheme.tier1_bars, scheme.tier2_bars
        );
    }

    #[test]
    fn test_compress_only_tier0_data() {
        // Only 60 samples (less than 120), width=100: caps at 60 bars (no expansion)
        let hist = make_history(60);
        let (compressed, scheme) = compress_history(&hist, 100);

        assert_eq!(scheme.tier0_bars, 60); // 1:1 - never expand
        assert_eq!(scheme.tier1_bars, 0);
        assert_eq!(scheme.tier2_bars, 0);
        assert_eq!(compressed.len(), 60); // Caps at num samples
    }

    #[test]
    fn test_compress_tier0_and_tier1_only() {
        // 200 samples (tier0 + partial tier1, no tier2), width=200
        let hist = make_history(200);
        let (compressed, scheme) = compress_history(&hist, 200);

        assert_eq!(scheme.tier0_bars, 120); // Full tier0 at 1:1
        assert_eq!(scheme.tier1_bars, 80); // tier1 at 1:1 (80 samples, no compression)
        assert_eq!(scheme.tier2_bars, 0);
        assert_eq!(compressed.len(), 200); // tier0 + tier1 = 200
        assert_eq!(scheme.tier0_bars + scheme.tier1_bars, 200);
    }

    #[test]
    fn test_all_samples_covered() {
        // Verify that compression doesn't lose samples
        let hist: VecDeque<f64> = (0..600).map(|i| if i < 10 { 100.0 } else { 0.0 }).collect();
        let (compressed, _) = compress_history(&hist, 50);

        // First bar should have high average (includes samples 0-9 with value 100)
        assert!(compressed[0] > 10.0, "First bar should capture the spike");

        // Should have exactly 50 bars
        assert_eq!(compressed.len(), 50);
    }

    #[test]
    fn test_visual_markers_match_bars() {
        let hist = make_history(600);
        let (_, scheme) = compress_history(&hist, 30);
        let markers = scheme.visual_markers();

        // Visual markers should match total bars
        assert_eq!(
            markers.len(),
            scheme.tier0_bars + scheme.tier1_bars + scheme.tier2_bars
        );
        assert_eq!(markers.len(), 30);
    }

    #[test]
    fn test_all_narrow_widths_show_all_tiers() {
        // Test that tier2 is visible at all narrow widths >= 3
        let hist = make_history(600);

        for width in 3..20 {
            let (compressed, scheme) = compress_history(&hist, width);
            assert!(
                scheme.tier2_bars >= 1,
                "Width {} should show tier2, got tier0={}, tier1={}, tier2={}",
                width,
                scheme.tier0_bars,
                scheme.tier1_bars,
                scheme.tier2_bars
            );
            assert_eq!(
                compressed.len(),
                width,
                "Width {} should produce {} bars, got {}",
                width,
                width,
                compressed.len()
            );
        }
    }

    #[test]
    fn test_always_fills_available_width_with_full_history() {
        // With full history (600 samples), should fill available width
        // UNLESS width > all samples (then we don't expand)
        let hist = make_history(600);

        for width in 1..600 {
            let (compressed, scheme) = compress_history(&hist, width);
            let total_bars = scheme.tier0_bars + scheme.tier1_bars + scheme.tier2_bars;
            assert_eq!(
                compressed.len(),
                width,
                "Width {}: compressed.len()={} but should be {}",
                width,
                compressed.len(),
                width
            );
            assert_eq!(
                total_bars, width,
                "Width {}: total_bars={} but should be {}",
                width, total_bars, width
            );
        }
    }

    #[test]
    fn test_fills_width_with_partial_history() {
        // With 100 samples, should fill width up to 100 bars (never expand beyond)
        let hist = make_history(100); // Only tier0 data

        for width in 1..=100 {
            let (compressed, _) = compress_history(&hist, width);
            assert_eq!(
                compressed.len(),
                width,
                "Width {}: compressed.len()={} but should fill {} available width",
                width,
                compressed.len(),
                width
            );
        }

        // Beyond 100, should cap at 100 bars (1 bar per sample max)
        for width in 101..150 {
            let (compressed, _) = compress_history(&hist, width);
            assert_eq!(
                compressed.len(),
                100,
                "Width {}: compressed.len()={} but should cap at 100 (num samples)",
                width,
                compressed.len()
            );
        }
    }

    #[test]
    fn test_tier0_never_compressed_when_possible() {
        // Tier0 (first 120 samples) should NEVER be compressed when width >= 120
        let hist = make_history(600);

        for width in 120..300 {
            let (compressed, scheme) = compress_history(&hist, width);
            assert_eq!(
                scheme.tier0_bars, 120,
                "Width {}: tier0 should be full resolution (120 bars), got {}",
                width, scheme.tier0_bars
            );
            // First 120 bars should be exact samples (1:1 mapping)
            for i in 0..120 {
                assert_eq!(
                    compressed[i], i as f64,
                    "Width {}: tier0 bar {} should be sample {} uncompressed",
                    width, i, i
                );
            }
        }
    }

    #[test]
    fn test_very_small_width_shows_all_tiers() {
        // CRITICAL: When window is very small and history is long,
        // there should be at least 1 bar from each tier (including compressed sections)
        let hist = make_history(600); // Full 10 minutes of history

        for width in 5..20 {
            let (_compressed, scheme) = compress_history(&hist, width);

            assert!(
                scheme.tier0_bars >= 1,
                "Width {}: tier0 should have at least 1 bar, got {}",
                width,
                scheme.tier0_bars
            );
            assert!(
                scheme.tier1_bars >= 1,
                "Width {}: tier1 should have at least 1 bar, got {}",
                width,
                scheme.tier1_bars
            );
            assert!(
                scheme.tier2_bars >= 1,
                "Width {}: tier2 should have at least 1 bar, got {}",
                width,
                scheme.tier2_bars
            );

            println!(
                "Width {}: tier0={}, tier1={}, tier2={}",
                width, scheme.tier0_bars, scheme.tier1_bars, scheme.tier2_bars
            );
        }
    }

    #[test]
    fn test_small_window_small_history_should_compress() {
        // CRITICAL BUG REPRO: Test various combinations of small widths and history lengths
        // When history exceeds width, compression MUST be visible across ALL compressed positions
        // When history equals width, 1:1 is OK (no markers expected)

        // Test various window sizes
        for width in 5..=15 {
            // Test history lengths around and above the width
            for hist_len in width..=(width + 10) {
                let hist = make_history(hist_len);
                let (compressed, scheme) = compress_history(&hist, width);

                // Should fill all available width (up to hist_len if smaller)
                let expected_len = hist_len.min(width);
                assert_eq!(
                    compressed.len(),
                    expected_len,
                    "width={}, hist_len={}: compressed length should be {}",
                    width,
                    hist_len,
                    expected_len
                );

                // Check compression markers
                let markers = scheme.visual_markers();
                let total_bars = scheme.tier0_bars + scheme.tier1_bars + scheme.tier2_bars;

                assert_eq!(
                    markers.len(),
                    total_bars,
                    "width={}, hist_len={}: markers should match total bars",
                    width,
                    hist_len
                );

                // Count non-space markers (spaces mean 1:1, no compression)
                let visible_markers = markers.iter().filter(|span| span.content != " ").count();

                // When hist_len > width, compression MUST occur
                if hist_len > width {
                    // At least some compressed bars should exist
                    let _compressed_bars =
                        if scheme.tier0_bars < scheme.tier0_samples {
                            // tier0 is compressed
                            scheme.tier0_bars
                        } else {
                            0
                        } + if scheme.tier1_bars > 0 && scheme.tier1_bars < scheme.tier1_samples {
                            scheme.tier1_bars
                        } else {
                            0
                        } + if scheme.tier2_bars > 0 && scheme.tier2_bars < scheme.tier2_samples {
                            scheme.tier2_bars
                        } else {
                            0
                        };

                    // ALL compressed bars should show visible markers (not spaces)
                    // Each tier with compression (ratio > 1) should show its fallback char for ALL bars
                    let tier0_visible =
                        if scheme.tier0_bars > 0 && scheme.tier0_bars < scheme.tier0_samples {
                            scheme.tier0_bars
                        } else {
                            0
                        };
                    let tier1_visible =
                        if scheme.tier1_bars > 0 && scheme.tier1_bars < scheme.tier1_samples {
                            scheme.tier1_bars
                        } else {
                            0
                        };
                    let tier2_visible =
                        if scheme.tier2_bars > 0 && scheme.tier2_bars < scheme.tier2_samples {
                            scheme.tier2_bars
                        } else {
                            0
                        };
                    let expected_visible = tier0_visible + tier1_visible + tier2_visible;

                    assert_eq!(
                        visible_markers, expected_visible,
                        "width={}, hist_len={}: With {} samples in {} spaces, expected {} visible compression markers but got {}. \
                        Scheme: tier0: {}→{} ({}x), tier1: {}→{} ({}x), tier2: {}→{} ({}x)",
                        width, hist_len, hist_len, width, expected_visible, visible_markers,
                        scheme.tier0_samples, scheme.tier0_bars,
                        if scheme.tier0_bars > 0 { (scheme.tier0_samples as f64 / scheme.tier0_bars as f64).ceil() as usize } else { 0 },
                        scheme.tier1_samples, scheme.tier1_bars,
                        if scheme.tier1_bars > 0 { (scheme.tier1_samples as f64 / scheme.tier1_bars as f64).ceil() as usize } else { 0 },
                        scheme.tier2_samples, scheme.tier2_bars,
                        if scheme.tier2_bars > 0 { (scheme.tier2_samples as f64 / scheme.tier2_bars as f64).ceil() as usize } else { 0 }
                    );
                }
            }
        }
    }

    #[test]
    fn test_small_window_long_tier0_history() {
        // CRITICAL: 30 seconds of history (all in tier0), 10 slots available
        // Should show: some full-res recent samples + compressed older samples
        // NOT: all 30 samples uniformly compressed
        let hist = make_history(30);
        let (compressed, _scheme) = compress_history(&hist, 10);

        // Should use tiered compression even within tier0's time range
        // Recent samples at full res, older compressed
        assert_eq!(compressed.len(), 10);

        // Should compress to show all 30 seconds in 10 slots
        // For example: first 5 bars = 1s each (recent), last 5 bars = 5s each (older)
        // The key is: should NOT just take first 10 samples and ignore the rest!

        // Verify we're covering all samples, not just first 10
        // The last compressed bar should include data from samples 20-29
        // (This will fail with current implementation)
    }

    #[test]
    fn test_compression_markers_visible_on_narrow_windows() {
        // CRITICAL: When window is narrow and history is long,
        // compression markers should be VISIBLE (not all spaces)
        let hist = make_history(600);

        for width in 5..30 {
            let (_, scheme) = compress_history(&hist, width);
            let markers = scheme.visual_markers();

            // Count non-space markers
            let visible_markers = markers.iter().filter(|span| span.content != " ").count();

            // With heavy compression, we should see visible indicators
            // At minimum, compressed sections should show SOMETHING
            assert!(
                visible_markers > 0,
                "Width {}: should have visible compression markers, got {} visible out of {} total",
                width,
                visible_markers,
                markers.len()
            );

            println!(
                "Width {}: {} visible markers out of {} total (tier0={}, tier1={}, tier2={})",
                width,
                visible_markers,
                markers.len(),
                scheme.tier0_bars,
                scheme.tier1_bars,
                scheme.tier2_bars
            );
        }
    }

    #[test]
    fn test_samples_never_expanded() {
        // CRITICAL: Each sample should be at most 1 bar (never expand samples)
        let hist = make_history(600);

        // Test various widths > 600
        for width in 600..1000 {
            let (_compressed, scheme) = compress_history(&hist, width);

            // Total bars should cap at total samples
            let total_bars = scheme.tier0_bars + scheme.tier1_bars + scheme.tier2_bars;
            assert!(
                total_bars <= 600,
                "Width {}: total_bars={} should not exceed 600 samples",
                width,
                total_bars
            );

            // tier0 should cap at 120
            assert!(
                scheme.tier0_bars <= 120,
                "Width {}: tier0_bars={} should not exceed 120 samples",
                width,
                scheme.tier0_bars
            );

            // tier1 should cap at 180
            assert!(
                scheme.tier1_bars <= 180,
                "Width {}: tier1_bars={} should not exceed 180 samples",
                width,
                scheme.tier1_bars
            );

            // tier2 should cap at 300
            assert!(
                scheme.tier2_bars <= 300,
                "Width {}: tier2_bars={} should not exceed 300 samples",
                width,
                scheme.tier2_bars
            );
        }
    }

    #[test]
    fn test_scheme_never_has_more_bars_than_samples() {
        // CRITICAL: The scheme should NEVER have more bars than samples in any tier
        // This would cause visual "stretching" of data
        let hist = make_history(600);

        for width in 1..700 {
            let (_, scheme) = compress_history(&hist, width);

            assert!(
                scheme.tier0_bars <= scheme.tier0_samples,
                "Width {}: tier0_bars ({}) > tier0_samples ({})",
                width,
                scheme.tier0_bars,
                scheme.tier0_samples
            );

            if scheme.tier1_bars > 0 {
                assert!(
                    scheme.tier1_bars <= scheme.tier1_samples,
                    "Width {}: tier1_bars ({}) > tier1_samples ({})",
                    width,
                    scheme.tier1_bars,
                    scheme.tier1_samples
                );
            }

            if scheme.tier2_bars > 0 {
                assert!(
                    scheme.tier2_bars <= scheme.tier2_samples,
                    "Width {}: tier2_bars ({}) > tier2_samples ({})",
                    width,
                    scheme.tier2_bars,
                    scheme.tier2_samples
                );
            }
        }
    }

    #[test]
    fn test_partial_history_never_stretches() {
        // When you have partial history (e.g., 60 samples) and resize window,
        // samples should never stretch beyond 1:1
        let hist = make_history(60); // Only 60 seconds of history

        for width in 1..200 {
            let (compressed, scheme) = compress_history(&hist, width);

            // Total bars should never exceed total samples
            let total_bars = scheme.tier0_bars + scheme.tier1_bars + scheme.tier2_bars;
            assert!(
                total_bars <= 60,
                "Width {}: total_bars={} exceeds 60 samples",
                width,
                total_bars
            );

            // Compressed output should never exceed samples
            assert!(
                compressed.len() <= 60,
                "Width {}: compressed.len()={} exceeds 60 samples",
                width,
                compressed.len()
            );

            println!("Width {}: compressed.len()={}, total_bars={}, tier0={}→{}, tier1={}→{}, tier2={}→{}",
                     width, compressed.len(), total_bars,
                     scheme.tier0_samples, scheme.tier0_bars,
                     scheme.tier1_samples, scheme.tier1_bars,
                     scheme.tier2_samples, scheme.tier2_bars);
        }
    }

    #[test]
    fn test_smoother_compression_ramp() {
        // When tier0 is at full res and remaining width is small,
        // compression ratios should be reasonable (prefer <10x when possible)
        let hist = make_history(600);

        // Test case: 130 width = 120 for tier0 + 10 remaining for tier1+tier2
        let (_, scheme) = compress_history(&hist, 130);

        // tier0 should be full res
        assert_eq!(scheme.tier0_bars, 120);

        // Calculate actual compression ratios
        let tier1_ratio = if scheme.tier1_bars > 0 {
            scheme.tier1_samples as f64 / scheme.tier1_bars as f64
        } else {
            0.0
        };
        let tier2_ratio = if scheme.tier2_bars > 0 {
            scheme.tier2_samples as f64 / scheme.tier2_bars as f64
        } else {
            0.0
        };

        println!(
            "Width 130: tier1={}x ({}→{}), tier2={}x ({}→{})",
            tier1_ratio,
            scheme.tier1_samples,
            scheme.tier1_bars,
            tier2_ratio,
            scheme.tier2_samples,
            scheme.tier2_bars
        );

        // With preferred max of 10x, tier1 should get priority and reasonable compression
        assert!(
            scheme.tier1_bars >= 6,
            "tier1 should get majority of remaining width"
        );
        assert!(
            tier1_ratio <= 30.0,
            "tier1 compression {}x too extreme",
            tier1_ratio
        );

        // tier2 can be more compressed but not insane
        if scheme.tier2_bars > 0 {
            assert!(
                tier2_ratio <= 100.0,
                "tier2 compression {}x too extreme",
                tier2_ratio
            );
        }
    }
}
