//! The unread badge on Windows' taskbar, drawn at the size the taskbar shows
//! it: 16 pixels at 100 %, more on denser screens. A mint disc, or a pill once
//! the count takes two places, rimmed in the tile's dark so it stands off the
//! icon and a light taskbar alike. The count is written in the mark's own
//! line, strokes with round ends; past 99 it reads "99+", as Windows' own
//! badges do.

use Stroke::{Arc, Line};

const MINT: [f32; 3] = [95.0, 208.0, 170.0];
const INK: [f32; 3] = [22.0, 24.0, 27.0];

/// RGBA, `size` × `size`, for a count of 1 or more.
pub(super) fn badge(count: u32, size: u32) -> Vec<u8> {
    let text = if count > 99 {
        "99+".to_owned()
    } else {
        count.to_string()
    };
    let glyphs: Vec<&Glyph> = text.chars().map(glyph).collect();
    let side = size as f32;
    let rim = (side / 16.0).max(1.0);

    // One figure sits in a disc; more, in a pill as wide as the badge, low in
    // it, where the disc would have touched the corner. "99+" is set tighter.
    let (tall, high, line) = match glyphs.len() {
        1 => (side, (side * 0.5).round(), (side * 0.12).round().max(2.0)),
        2 => (
            (side * 0.8125).round(),
            (side * 0.375).round(),
            (side * 0.12).round().max(2.0),
        ),
        _ => ((side * 0.8125).round(), (side * 0.3125).round(), (side * 0.09).max(1.5)),
    };
    let round = tall / 2.0;
    let middle = side - round;
    let ends = ([round, middle], [side - round, middle]);

    // The figures, centred; their tops and feet on whole pixels.
    let gap = line * if glyphs.len() > 2 { 0.3 } else { 0.4 };
    let inked: f32 =
        glyphs.iter().map(|glyph| glyph.width * high + line).sum::<f32>() + gap * (glyphs.len() - 1) as f32;
    let top = (middle - high / 2.0 - line / 2.0).round() + line / 2.0;
    let mut left = (side - inked) / 2.0 + line / 2.0;
    let mut strokes = Vec::new();
    for glyph in &glyphs {
        let at = |[x, y]: [f32; 2]| [left + x * high, top + y * high];
        for stroke in glyph.strokes {
            strokes.push(match *stroke {
                Line(from, to) => Line(at(from), at(to)),
                Arc(centre, radius, from, sweep) => Arc(at(centre), radius * high, from, sweep),
            });
        }
        left += glyph.width * high + line + gap;
    }

    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let point = [x as f32 + 0.5, y as f32 + 0.5];
            let edge = segment_distance(point, ends.0, ends.1) - round;
            let shape = coverage(-edge);
            let inside = coverage(-edge - rim);
            let nearest = strokes
                .iter()
                .map(|stroke| stroke.distance(point))
                .fold(f32::INFINITY, f32::min);
            let figure = coverage(line / 2.0 - nearest);
            let mint = inside * (1.0 - figure);
            for channel in 0..3 {
                let value = INK[channel] + (MINT[channel] - INK[channel]) * mint;
                pixels.push(value.round() as u8);
            }
            pixels.push((shape * 255.0).round() as u8);
        }
    }
    pixels
}

/// How much of a pixel lies inside an edge `depth` pixels away from its
/// centre (negative: outside).
fn coverage(depth: f32) -> f32 {
    (depth + 0.5).clamp(0.0, 1.0)
}

fn segment_distance(point: [f32; 2], from: [f32; 2], to: [f32; 2]) -> f32 {
    let along = [to[0] - from[0], to[1] - from[1]];
    let towards = [point[0] - from[0], point[1] - from[1]];
    let length = along[0] * along[0] + along[1] * along[1];
    let share = if length == 0.0 {
        0.0
    } else {
        ((towards[0] * along[0] + towards[1] * along[1]) / length).clamp(0.0, 1.0)
    };
    distance(point, [from[0] + along[0] * share, from[1] + along[1] * share])
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// A figure, one unit high (stroke centres from top to foot), drawn in lines
/// and circular arcs; y grows downwards.
struct Glyph {
    width: f32,
    strokes: &'static [Stroke],
}

#[derive(Clone, Copy)]
enum Stroke {
    Line([f32; 2], [f32; 2]),
    /// Centre, radius, start and sweep in degrees, clockwise from 3 o'clock.
    Arc([f32; 2], f32, f32, f32),
}

impl Stroke {
    fn distance(self, point: [f32; 2]) -> f32 {
        match self {
            Line(from, to) => segment_distance(point, from, to),
            Arc(centre, radius, from, sweep) => {
                let angle = (point[1] - centre[1]).atan2(point[0] - centre[0]).to_degrees();
                if (angle - from).rem_euclid(360.0) <= sweep {
                    (distance(point, centre) - radius).abs()
                } else {
                    let end = |degrees: f32| {
                        let radians = degrees.to_radians();
                        [centre[0] + radius * radians.cos(), centre[1] + radius * radians.sin()]
                    };
                    distance(point, end(from)).min(distance(point, end(from + sweep)))
                }
            }
        }
    }
}

fn glyph(figure: char) -> &'static Glyph {
    match figure {
        '0' => &ZERO,
        '1' => &ONE,
        '2' => &TWO,
        '3' => &THREE,
        '4' => &FOUR,
        '5' => &FIVE,
        '6' => &SIX,
        '7' => &SEVEN,
        '8' => &EIGHT,
        '9' => &NINE,
        _ => &PLUS,
    }
}

const ZERO: Glyph = Glyph {
    width: 0.6,
    strokes: &[
        Arc([0.3, 0.3], 0.3, 180.0, 180.0),
        Line([0.6, 0.3], [0.6, 0.7]),
        Arc([0.3, 0.7], 0.3, 0.0, 180.0),
        Line([0.0, 0.7], [0.0, 0.3]),
    ],
};
const ONE: Glyph = Glyph {
    width: 0.3,
    strokes: &[Line([0.0, 0.2], [0.3, 0.0]), Line([0.3, 0.0], [0.3, 1.0])],
};
const TWO: Glyph = Glyph {
    width: 0.6,
    strokes: &[
        Arc([0.3, 0.3], 0.3, 195.0, 195.0),
        Line([0.56, 0.45], [0.0, 1.0]),
        Line([0.0, 1.0], [0.6, 1.0]),
    ],
};
const THREE: Glyph = Glyph {
    width: 0.56,
    strokes: &[
        Arc([0.28, 0.24], 0.24, 195.0, 255.0),
        Line([0.14, 0.48], [0.28, 0.48]),
        Arc([0.28, 0.74], 0.26, 270.0, 255.0),
    ],
};
const FOUR: Glyph = Glyph {
    width: 0.6,
    strokes: &[
        Line([0.44, 1.0], [0.44, 0.0]),
        Line([0.44, 0.0], [0.0, 0.75]),
        Line([0.0, 0.75], [0.6, 0.75]),
    ],
};
const FIVE: Glyph = Glyph {
    width: 0.6,
    strokes: &[
        Line([0.56, 0.0], [0.07, 0.0]),
        Line([0.07, 0.0], [0.054, 0.528]),
        Arc([0.3, 0.7], 0.3, 215.0, 295.0),
    ],
};
const SIX: Glyph = Glyph {
    width: 0.6,
    strokes: &[Line([0.44, 0.0], [0.03, 0.6]), Arc([0.3, 0.7], 0.3, 0.0, 360.0)],
};
const SEVEN: Glyph = Glyph {
    width: 0.6,
    strokes: &[Line([0.0, 0.0], [0.6, 0.0]), Line([0.6, 0.0], [0.18, 1.0])],
};
const EIGHT: Glyph = Glyph {
    width: 0.56,
    strokes: &[Arc([0.28, 0.23], 0.23, 0.0, 360.0), Arc([0.28, 0.74], 0.26, 0.0, 360.0)],
};
const NINE: Glyph = Glyph {
    width: 0.6,
    strokes: &[Arc([0.3, 0.3], 0.3, 0.0, 360.0), Line([0.57, 0.4], [0.16, 1.0])],
};
const PLUS: Glyph = Glyph {
    width: 0.4,
    strokes: &[Line([0.2, 0.3], [0.2, 0.7]), Line([0.0, 0.5], [0.4, 0.5])],
};
