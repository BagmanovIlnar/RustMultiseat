//! Pointer-based drag-and-drop overlay (Stage 6, план §8).
//!
//! Полупрозрачный canvas-слой на всё окно:
//! - рисует бейдж с перетаскиваемым устройством рядом с курсором
//!   (визуальный preview);
//! - отменяет перетаскивание по Escape (`canvas::Action::publish`).
//!
//! Hit-testing источников и drop-зон выполняют `mouse_area`-обёртки в
//! `panels.rs`; pointer capture/состояние drag живёт в `DragState`
//! приложения. Drop меняет только `draft`, после чего выполняется локальный
//! preview-validation и валидация демона по команде Validate/Start.

use iced::alignment::Vertical;
use iced::keyboard::key::Named;
use iced::mouse;
use iced::widget::canvas::{self, Action, Frame, Path, Program, Stroke, Text};
use iced::widget::text::Alignment;
use iced::{Color, Font, Length, Point, Rectangle, Size};

use crate::Message;

/// Badge text font size (px).
const FONT_SIZE: f32 = 13.0;
/// Badge height (px).
const BADGE_HEIGHT: f32 = 26.0;
/// Badge horizontal padding (px).
const PAD_X: f32 = 10.0;
/// Badge offset from the cursor (px).
const OFFSET: f32 = 14.0;

const BADGE_BG: Color = Color::from_rgba(0.08, 0.09, 0.12, 0.95);
const BADGE_BORDER: Color = Color::from_rgb(0.36, 0.56, 0.95);

/// Dev-grade width estimate for the badge background.
///
/// Canvas `Frame` cannot measure text, so a fixed glyph ratio is used; the
/// badge is non-interactive, clipping by a few px is acceptable.
pub fn estimate_text_width(content: &str) -> f32 {
    content.chars().count() as f32 * FONT_SIZE * 0.62
}

/// Badge rectangle near `cursor`, clamped into `window` bounds.
pub fn badge_rectangle(cursor: Point, label: &str, window: &Rectangle) -> Rectangle {
    let size = Size::new(estimate_text_width(label) + 2.0 * PAD_X, BADGE_HEIGHT);
    let mut origin = Point::new(cursor.x + OFFSET, cursor.y + OFFSET);
    if origin.x + size.width > window.x + window.width {
        origin.x = (window.x + window.width - size.width).max(window.x);
    }
    if origin.y + size.height > window.y + window.height {
        origin.y = (window.y + window.height - size.height).max(window.y);
    }
    Rectangle::new(origin, size)
}

/// Full-window drag preview layer; `label` empty means idle (draws nothing).
#[derive(Debug, Clone)]
pub struct DragPreview {
    label: String,
    on_cancel: Message,
}

impl DragPreview {
    /// Creates the layer for the dragged item label (empty = idle).
    pub fn new(label: String, on_cancel: Message) -> Self {
        Self { label, on_cancel }
    }

    fn active(&self) -> bool {
        !self.label.is_empty()
    }
}

impl Program<Message> for DragPreview {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &iced::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        if !self.active() {
            return None;
        }
        let escape_pressed = matches!(
            event,
            iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(Named::Escape),
                ..
            })
        );
        escape_pressed.then(|| Action::publish(self.on_cancel.clone()))
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry<iced::Renderer>> {
        if !self.active() {
            return Vec::new();
        }
        let Some(position) = cursor.position() else {
            return Vec::new();
        };

        let rect = badge_rectangle(position, &self.label, &bounds);
        let mut frame = Frame::new(renderer, bounds.size());

        let badge = Path::rounded_rectangle(Point::new(rect.x, rect.y), rect.size(), 8.0.into());
        frame.fill(&badge, BADGE_BG);
        frame.stroke(
            &badge,
            Stroke {
                style: canvas::Style::Solid(BADGE_BORDER),
                width: 1.5,
                ..Stroke::default()
            },
        );
        frame.fill_text(Text {
            content: self.label.clone(),
            position: rect.center(),
            color: Color::WHITE,
            size: FONT_SIZE.into(),
            font: Font::DEFAULT,
            align_x: Alignment::Center,
            align_y: Vertical::Center,
            ..Text::default()
        });

        vec![frame.into_geometry()]
    }
}

/// Convenience constructor for the full-window overlay canvas.
///
/// The canvas is event-transparent: `Program::update` only observes the
/// Escape key, everything else stays `Ignored` and reaches the widgets
/// underneath (sources / drop zones).
pub fn overlay_canvas(preview: DragPreview) -> iced::widget::Canvas<DragPreview, Message> {
    iced::widget::canvas(preview)
        .width(Length::Fill)
        .height(Length::Fill)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window() -> Rectangle {
        Rectangle::new(Point::new(0.0, 0.0), Size::new(1280.0, 800.0))
    }

    #[test]
    fn text_width_is_monotonic_and_nonzero() {
        assert_eq!(estimate_text_width(""), 0.0);
        assert!(estimate_text_width("kbd KBD1") > estimate_text_width("kbd"));
    }

    #[test]
    fn badge_follows_cursor_when_inside_window() {
        let rect = badge_rectangle(Point::new(100.0, 100.0), "kbd KBD1", &window());
        assert_eq!(rect.x, 100.0 + OFFSET);
        assert_eq!(rect.y, 100.0 + OFFSET);
        assert!(rect.width > 2.0 * PAD_X);
        assert_eq!(rect.height, BADGE_HEIGHT);
    }

    #[test]
    fn badge_clamps_to_right_and_bottom_edges() {
        let rect = badge_rectangle(
            Point::new(1270.0, 790.0),
            "display \\\\.\\DISPLAY2",
            &window(),
        );
        assert!(rect.x + rect.width <= window().width + f32::EPSILON);
        assert!(rect.y + rect.height <= window().height + f32::EPSILON);
        assert!(rect.x >= 0.0 && rect.y >= 0.0);
    }

    #[test]
    fn badge_clamps_even_in_tiny_window() {
        let tiny = Rectangle::new(Point::new(0.0, 0.0), Size::new(50.0, 20.0));
        let rect = badge_rectangle(Point::new(10.0, 10.0), "ptr PTR1", &tiny);
        assert!(rect.x >= 0.0);
        assert!(rect.y >= 0.0);
        // Width may overflow a tiny window; the badge is non-interactive.
    }
}
