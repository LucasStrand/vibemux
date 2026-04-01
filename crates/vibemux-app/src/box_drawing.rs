//! Custom geometric rendering for box-drawing (U+2500–U+257F) and block
//! element (U+2580–U+259F) characters.
//!
//! Instead of relying on font glyphs (which leave visible gaps between cells),
//! we detect these characters and draw them as pixel-perfect geometric
//! primitives on an Iced Canvas overlay.

use iced::widget::canvas::{self, Fill as CanvasFill, Frame, Geometry, Path, Stroke};
use iced::{Color, Rectangle, Renderer, Theme};

/// Line weight for a given direction.
#[derive(Clone, Copy, PartialEq, Eq)]
enum W {
    /// No line in this direction.
    N,
    /// Light (thin) line.
    L,
    /// Heavy (thick) line.
    H,
    /// Double line.
    D,
}

/// Directional segments: (left, right, up, down).
type Seg = (W, W, W, W);

/// A box-drawing or block-element character to be rendered on the canvas.
#[derive(Clone)]
pub struct BoxDrawCell {
    pub row: usize,
    pub col: usize,
    pub ch: char,
    pub fg: Color,
    pub bg: Color,
}

/// Canvas program that draws box-drawing characters as geometric primitives.
pub struct BoxDrawingOverlay {
    pub cells: Vec<BoxDrawCell>,
    pub cell_width: f32,
    pub cell_height: f32,
    pub padding: f32,
}

impl<Message> canvas::Program<Message> for BoxDrawingOverlay {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());

        for cell in &self.cells {
            let x = cell.col as f32 * self.cell_width + self.padding;
            let y = cell.row as f32 * self.cell_height + self.padding;
            let w = self.cell_width;
            let h = self.cell_height;

            if let Some(seg) = segments(cell.ch) {
                draw_box_char(&mut frame, x, y, w, h, seg, cell.fg);
            } else if is_block_element(cell.ch) {
                draw_block_element(&mut frame, x, y, w, h, cell.ch, cell.fg, cell.bg);
            }
        }

        vec![frame.into_geometry()]
    }
}

/// Returns `true` if `ch` is a box-drawing or block-element character we handle.
pub fn is_box_drawing(ch: char) -> bool {
    ('\u{2500}'..='\u{259F}').contains(&ch)
}

/// Returns `true` if `ch` is a block-element character (U+2580–U+259F).
fn is_block_element(ch: char) -> bool {
    ('\u{2580}'..='\u{259F}').contains(&ch)
}

/// Map a box-drawing character to its directional segments.
fn segments(ch: char) -> Option<Seg> {
    use W::*;
    let s = match ch {
        // ─ light horizontals & verticals
        '\u{2500}' => (L, L, N, N), // ─
        '\u{2501}' => (H, H, N, N), // ━
        '\u{2502}' => (N, N, L, L), // │
        '\u{2503}' => (N, N, H, H), // ┃

        // dashed / triple / quadruple horizontals – treat as light/heavy
        '\u{2504}' => (L, L, N, N), // ┄
        '\u{2505}' => (H, H, N, N), // ┅
        '\u{2506}' => (N, N, L, L), // ┆
        '\u{2507}' => (N, N, H, H), // ┇
        '\u{2508}' => (L, L, N, N), // ┈
        '\u{2509}' => (H, H, N, N), // ┉
        '\u{250A}' => (N, N, L, L), // ┊
        '\u{250B}' => (N, N, H, H), // ┋

        // corners: down-and-right
        '\u{250C}' => (N, L, N, L), // ┌
        '\u{250D}' => (N, H, N, L), // ┍
        '\u{250E}' => (N, L, N, H), // ┎
        '\u{250F}' => (N, H, N, H), // ┏

        // corners: down-and-left
        '\u{2510}' => (L, N, N, L), // ┐
        '\u{2511}' => (H, N, N, L), // ┑
        '\u{2512}' => (L, N, N, H), // ┒
        '\u{2513}' => (H, N, N, H), // ┓

        // corners: up-and-right
        '\u{2514}' => (N, L, L, N), // └
        '\u{2515}' => (N, H, L, N), // ┕
        '\u{2516}' => (N, L, H, N), // ┖
        '\u{2517}' => (N, H, H, N), // ┗

        // corners: up-and-left
        '\u{2518}' => (L, N, L, N), // ┘
        '\u{2519}' => (H, N, L, N), // ┙
        '\u{251A}' => (L, N, H, N), // ┚
        '\u{251B}' => (H, N, H, N), // ┛

        // tees: vertical-and-right
        '\u{251C}' => (N, L, L, L), // ├
        '\u{251D}' => (N, H, L, L), // ┝
        '\u{251E}' => (N, L, H, L), // ┞
        '\u{251F}' => (N, L, L, H), // ┟
        '\u{2520}' => (N, L, H, H), // ┠
        '\u{2521}' => (N, H, H, L), // ┡
        '\u{2522}' => (N, H, L, H), // ┢
        '\u{2523}' => (N, H, H, H), // ┣

        // tees: vertical-and-left
        '\u{2524}' => (L, N, L, L), // ┤
        '\u{2525}' => (H, N, L, L), // ┥
        '\u{2526}' => (L, N, H, L), // ┦
        '\u{2527}' => (L, N, L, H), // ┧
        '\u{2528}' => (L, N, H, H), // ┨
        '\u{2529}' => (H, N, H, L), // ┩
        '\u{252A}' => (H, N, L, H), // ┪
        '\u{252B}' => (H, N, H, H), // ┫

        // tees: horizontal-and-down
        '\u{252C}' => (L, L, N, L), // ┬
        '\u{252D}' => (H, L, N, L), // ┭
        '\u{252E}' => (L, H, N, L), // ┮
        '\u{252F}' => (H, H, N, L), // ┯
        '\u{2530}' => (L, L, N, H), // ┰
        '\u{2531}' => (H, L, N, H), // ┱
        '\u{2532}' => (L, H, N, H), // ┲
        '\u{2533}' => (H, H, N, H), // ┳

        // tees: horizontal-and-up
        '\u{2534}' => (L, L, L, N), // ┴
        '\u{2535}' => (H, L, L, N), // ┵
        '\u{2536}' => (L, H, L, N), // ┶
        '\u{2537}' => (H, H, L, N), // ┷
        '\u{2538}' => (L, L, H, N), // ┸
        '\u{2539}' => (H, L, H, N), // ┹
        '\u{253A}' => (L, H, H, N), // ┺
        '\u{253B}' => (H, H, H, N), // ┻

        // crosses
        '\u{253C}' => (L, L, L, L), // ┼
        '\u{253D}' => (H, L, L, L), // ┽
        '\u{253E}' => (L, H, L, L), // ┾
        '\u{253F}' => (H, H, L, L), // ┿
        '\u{2540}' => (L, L, H, L), // ╀
        '\u{2541}' => (L, L, L, H), // ╁
        '\u{2542}' => (L, L, H, H), // ╂
        '\u{2543}' => (H, L, H, L), // ╃
        '\u{2544}' => (L, H, H, L), // ╄
        '\u{2545}' => (H, L, L, H), // ╅
        '\u{2546}' => (L, H, L, H), // ╆
        '\u{2547}' => (H, H, H, L), // ╇
        '\u{2548}' => (H, H, L, H), // ╈
        '\u{2549}' => (H, L, H, H), // ╉
        '\u{254A}' => (L, H, H, H), // ╊
        '\u{254B}' => (H, H, H, H), // ╋

        // double lines
        '\u{2550}' => (D, D, N, N), // ═
        '\u{2551}' => (N, N, D, D), // ║
        '\u{2552}' => (N, D, N, L), // ╒
        '\u{2553}' => (N, L, N, D), // ╓
        '\u{2554}' => (N, D, N, D), // ╔
        '\u{2555}' => (D, N, N, L), // ╕
        '\u{2556}' => (L, N, N, D), // ╖
        '\u{2557}' => (D, N, N, D), // ╗
        '\u{2558}' => (N, D, L, N), // ╘
        '\u{2559}' => (N, L, D, N), // ╙
        '\u{255A}' => (N, D, D, N), // ╚
        '\u{255B}' => (D, N, L, N), // ╛
        '\u{255C}' => (L, N, D, N), // ╜
        '\u{255D}' => (D, N, D, N), // ╝
        '\u{255E}' => (N, D, L, L), // ╞
        '\u{255F}' => (N, L, D, D), // ╟
        '\u{2560}' => (N, D, D, D), // ╠
        '\u{2561}' => (D, N, L, L), // ╡
        '\u{2562}' => (L, N, D, D), // ╢
        '\u{2563}' => (D, N, D, D), // ╣
        '\u{2564}' => (D, D, N, L), // ╤
        '\u{2565}' => (L, L, N, D), // ╥
        '\u{2566}' => (D, D, N, D), // ╦
        '\u{2567}' => (D, D, L, N), // ╧
        '\u{2568}' => (L, L, D, N), // ╨
        '\u{2569}' => (D, D, D, N), // ╩
        '\u{256A}' => (D, D, L, L), // ╪
        '\u{256B}' => (L, L, D, D), // ╫
        '\u{256C}' => (D, D, D, D), // ╬

        // rounded corners
        '\u{256D}' => (N, L, N, L), // ╭
        '\u{256E}' => (L, N, N, L), // ╮
        '\u{256F}' => (L, N, L, N), // ╯
        '\u{2570}' => (N, L, L, N), // ╰

        // diagonals – approximate as light cross
        '\u{2571}' => (L, L, L, L), // ╱  (not ideal but visible)
        '\u{2572}' => (L, L, L, L), // ╲
        '\u{2573}' => (L, L, L, L), // ╳

        // half-lines
        '\u{2574}' => (L, N, N, N), // ╴ left
        '\u{2575}' => (N, N, L, N), // ╵ up
        '\u{2576}' => (N, L, N, N), // ╶ right
        '\u{2577}' => (N, N, N, L), // ╷ down
        '\u{2578}' => (H, N, N, N), // ╸ heavy left
        '\u{2579}' => (N, N, H, N), // ╹ heavy up
        '\u{257A}' => (N, H, N, N), // ╺ heavy right
        '\u{257B}' => (N, N, N, H), // ╻ heavy down
        '\u{257C}' => (L, H, N, N), // ╼ light left, heavy right
        '\u{257D}' => (N, N, L, H), // ╽ light up, heavy down
        '\u{257E}' => (H, L, N, N), // ╾ heavy left, light right
        '\u{257F}' => (N, N, H, L), // ╿ heavy up, light down

        _ => return None,
    };
    Some(s)
}

/// Draw a block element character as a filled rectangle.
fn draw_block_element(
    frame: &mut Frame,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    ch: char,
    fg: Color,
    _bg: Color,
) {
    // Each block element covers a specific portion of the cell.
    // We express it as (x_offset, y_offset, width, height) in fractions of the cell.
    let (fx, fy, fw, fh) = match ch {
        '\u{2580}' => (0.0, 0.0, 1.0, 0.5),   // ▀ upper half
        '\u{2581}' => (0.0, 7.0/8.0, 1.0, 1.0/8.0), // ▁ lower 1/8
        '\u{2582}' => (0.0, 3.0/4.0, 1.0, 1.0/4.0), // ▂ lower 1/4
        '\u{2583}' => (0.0, 5.0/8.0, 1.0, 3.0/8.0), // ▃ lower 3/8
        '\u{2584}' => (0.0, 0.5, 1.0, 0.5),    // ▄ lower half
        '\u{2585}' => (0.0, 3.0/8.0, 1.0, 5.0/8.0), // ▅ lower 5/8
        '\u{2586}' => (0.0, 1.0/4.0, 1.0, 3.0/4.0), // ▆ lower 3/4
        '\u{2587}' => (0.0, 1.0/8.0, 1.0, 7.0/8.0), // ▇ lower 7/8
        '\u{2588}' => (0.0, 0.0, 1.0, 1.0),    // █ full block
        '\u{2589}' => (0.0, 0.0, 7.0/8.0, 1.0), // ▉ left 7/8
        '\u{258A}' => (0.0, 0.0, 3.0/4.0, 1.0), // ▊ left 3/4
        '\u{258B}' => (0.0, 0.0, 5.0/8.0, 1.0), // ▋ left 5/8
        '\u{258C}' => (0.0, 0.0, 0.5, 1.0),    // ▌ left half
        '\u{258D}' => (0.0, 0.0, 3.0/8.0, 1.0), // ▍ left 3/8
        '\u{258E}' => (0.0, 0.0, 1.0/4.0, 1.0), // ▎ left 1/4
        '\u{258F}' => (0.0, 0.0, 1.0/8.0, 1.0), // ▏ left 1/8
        '\u{2590}' => (0.5, 0.0, 0.5, 1.0),    // ▐ right half
        '\u{2591}' => (0.0, 0.0, 1.0, 1.0),    // ░ light shade (approximate)
        '\u{2592}' => (0.0, 0.0, 1.0, 1.0),    // ▒ medium shade
        '\u{2593}' => (0.0, 0.0, 1.0, 1.0),    // ▓ dark shade
        '\u{2594}' => (0.0, 0.0, 1.0, 1.0/8.0), // ▔ upper 1/8
        '\u{2595}' => (7.0/8.0, 0.0, 1.0/8.0, 1.0), // ▕ right 1/8
        '\u{2596}' => (0.0, 0.5, 0.5, 0.5),    // ▖ quadrant lower left
        '\u{2597}' => (0.5, 0.5, 0.5, 0.5),    // ▗ quadrant lower right
        '\u{2598}' => (0.0, 0.0, 0.5, 0.5),    // ▘ quadrant upper left
        '\u{2599}' => return draw_quadrants(frame, x, y, w, h, fg, true, false, true, true),
        '\u{259A}' => return draw_quadrants(frame, x, y, w, h, fg, true, false, false, true),
        '\u{259B}' => return draw_quadrants(frame, x, y, w, h, fg, true, true, true, false),
        '\u{259C}' => return draw_quadrants(frame, x, y, w, h, fg, true, true, false, true),
        '\u{259D}' => (0.5, 0.0, 0.5, 0.5),    // ▝ quadrant upper right
        '\u{259E}' => return draw_quadrants(frame, x, y, w, h, fg, false, true, true, false),
        '\u{259F}' => return draw_quadrants(frame, x, y, w, h, fg, false, true, true, true),
        _ => return,
    };

    // For shade characters, use reduced opacity.
    let color = match ch {
        '\u{2591}' => Color { a: fg.a * 0.25, ..fg },
        '\u{2592}' => Color { a: fg.a * 0.50, ..fg },
        '\u{2593}' => Color { a: fg.a * 0.75, ..fg },
        _ => fg,
    };

    let rect = Path::rectangle(
        iced::Point::new(x + fx * w, y + fy * h),
        iced::Size::new(fw * w, fh * h),
    );
    frame.fill(&rect, CanvasFill::from(color));
}

/// Draw multiple quadrants of the cell.
fn draw_quadrants(
    frame: &mut Frame,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: Color,
    upper_left: bool,
    upper_right: bool,
    lower_left: bool,
    lower_right: bool,
) {
    let hw = w / 2.0;
    let hh = h / 2.0;
    let fill = CanvasFill::from(color);
    if upper_left {
        frame.fill(&Path::rectangle(iced::Point::new(x, y), iced::Size::new(hw, hh)), fill);
    }
    if upper_right {
        frame.fill(&Path::rectangle(iced::Point::new(x + hw, y), iced::Size::new(hw, hh)), fill);
    }
    if lower_left {
        frame.fill(&Path::rectangle(iced::Point::new(x, y + hh), iced::Size::new(hw, hh)), fill);
    }
    if lower_right {
        frame.fill(&Path::rectangle(iced::Point::new(x + hw, y + hh), iced::Size::new(hw, hh)), fill);
    }
}

const LIGHT_WIDTH: f32 = 1.0;
const HEAVY_WIDTH: f32 = 2.0;
const DOUBLE_GAP: f32 = 2.0;

fn draw_box_char(
    frame: &mut Frame,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    (left, right, up, down): Seg,
    color: Color,
) {
    let cx = x + w / 2.0;
    let cy = y + h / 2.0;

    // Helper to draw a single line segment.
    let mut line = |x1: f32, y1: f32, x2: f32, y2: f32, width: f32| {
        let path = Path::line(iced::Point::new(x1, y1), iced::Point::new(x2, y2));
        frame.stroke(
            &path,
            Stroke::default().with_color(color).with_width(width),
        );
    };

    // --- Horizontal segments ---
    // Left
    match left {
        W::L => line(x, cy, cx, cy, LIGHT_WIDTH),
        W::H => line(x, cy, cx, cy, HEAVY_WIDTH),
        W::D => {
            line(x, cy - DOUBLE_GAP, cx, cy - DOUBLE_GAP, LIGHT_WIDTH);
            line(x, cy + DOUBLE_GAP, cx, cy + DOUBLE_GAP, LIGHT_WIDTH);
        }
        W::N => {}
    }
    // Right
    match right {
        W::L => line(cx, cy, x + w, cy, LIGHT_WIDTH),
        W::H => line(cx, cy, x + w, cy, HEAVY_WIDTH),
        W::D => {
            line(cx, cy - DOUBLE_GAP, x + w, cy - DOUBLE_GAP, LIGHT_WIDTH);
            line(cx, cy + DOUBLE_GAP, x + w, cy + DOUBLE_GAP, LIGHT_WIDTH);
        }
        W::N => {}
    }
    // Up
    match up {
        W::L => line(cx, y, cx, cy, LIGHT_WIDTH),
        W::H => line(cx, y, cx, cy, HEAVY_WIDTH),
        W::D => {
            line(cx - DOUBLE_GAP, y, cx - DOUBLE_GAP, cy, LIGHT_WIDTH);
            line(cx + DOUBLE_GAP, y, cx + DOUBLE_GAP, cy, LIGHT_WIDTH);
        }
        W::N => {}
    }
    // Down
    match down {
        W::L => line(cx, cy, cx, y + h, LIGHT_WIDTH),
        W::H => line(cx, cy, cx, y + h, HEAVY_WIDTH),
        W::D => {
            line(cx - DOUBLE_GAP, cy, cx - DOUBLE_GAP, y + h, LIGHT_WIDTH);
            line(cx + DOUBLE_GAP, cy, cx + DOUBLE_GAP, y + h, LIGHT_WIDTH);
        }
        W::N => {}
    }
}
