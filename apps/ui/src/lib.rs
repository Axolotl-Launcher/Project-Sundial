//! Superlogical UI component library, built on gpui.
//!
//! Presentation only: the crate knows how to draw labels, buttons, cards and
//! dividers, and nothing about application state or windowing. Wiring
//! components to state and opening a window belongs to `superlogical_frontend`.
//!
//! Components are props structs implementing [`RenderOnce`], so a caller builds
//! them inline on every frame and holds no entity of their own. The visual
//! palette arrives through the [`Theme`] global; callers install one at startup
//! with [`Theme::install`].

use gpui::{
    AnyElement, App, ClickEvent, ElementId, FontWeight, Global, Hsla, IntoElement, ParentElement,
    Pixels, RenderOnce, SharedString, Styled, Window, div, prelude::*, px, rgb,
};

/// The type scale shared by every component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSize {
    /// Small print and captions.
    Small,
    /// Body copy.
    Body,
    /// A section heading.
    Heading,
    /// A page title.
    Title,
    /// The largest figure on a screen.
    Display,
}

impl TextSize {
    /// The point size on the scale, in pixels.
    pub fn size(self) -> Pixels {
        px(match self {
            TextSize::Small => 12.,
            TextSize::Body => 14.,
            TextSize::Heading => 18.,
            TextSize::Title => 24.,
            TextSize::Display => 32.,
        })
    }
}

/// Text with no interactivity.
#[derive(IntoElement)]
pub struct Label {
    text: SharedString,
    color: Option<Hsla>,
    weight: Option<FontWeight>,
    size: Option<TextSize>,
}

impl Label {
    /// A label showing `text`.
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            color: None,
            weight: None,
            size: None,
        }
    }

    /// Sets the text colour.
    pub fn color(mut self, color: impl Into<Hsla>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Sets the font weight.
    pub fn weight(mut self, weight: FontWeight) -> Self {
        self.weight = Some(weight);
        self
    }

    /// Sets the step of the type scale.
    pub fn size(mut self, size: TextSize) -> Self {
        self.size = Some(size);
        self
    }
}

impl RenderOnce for Label {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .when_some(self.size, |el, size| el.text_size(size.size()))
            .when_some(self.color, |el, color| el.text_color(color))
            .when_some(self.weight, |el, weight| el.font_weight(weight))
            .child(self.text)
    }
}

/// The visual weight of a button.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonVariant {
    /// Filled with the accent colour: the primary action on a screen.
    #[default]
    Primary,
    /// Outlined, for secondary actions.
    Secondary,
    /// Flat, for low-emphasis actions.
    Ghost,
}

impl ButtonVariant {
    fn background(self, theme: &Theme) -> Hsla {
        match self {
            ButtonVariant::Primary => theme.accent,
            ButtonVariant::Secondary | ButtonVariant::Ghost => theme.surface,
        }
    }

    fn hover_background(self, theme: &Theme) -> Hsla {
        match self {
            ButtonVariant::Primary => theme.accent_hover,
            ButtonVariant::Secondary | ButtonVariant::Ghost => theme.surface_hover,
        }
    }

    fn border(self, theme: &Theme) -> Hsla {
        match self {
            ButtonVariant::Primary => theme.accent,
            ButtonVariant::Secondary => theme.border,
            ButtonVariant::Ghost => theme.surface,
        }
    }

    fn foreground(self, theme: &Theme) -> Hsla {
        match self {
            ButtonVariant::Primary => theme.on_accent,
            ButtonVariant::Secondary | ButtonVariant::Ghost => theme.text,
        }
    }
}

/// The handler a [`Button`] runs when activated.
pub type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// A clickable button.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    variant: ButtonVariant,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

impl Button {
    /// A button labelled `label`. `id` must be unique among its siblings.
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            variant: ButtonVariant::default(),
            disabled: false,
            on_click: None,
        }
    }

    /// Sets the visual weight.
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Whether the button ignores clicks and renders dimmed.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Called when the button is activated, by mouse or keyboard.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::global(cx);
        let variant = self.variant;
        let disabled = self.disabled;

        let mut button = div()
            .id(self.id)
            .flex()
            .flex_row()
            .items_center()
            .justify_center()
            .h(px(36.))
            .px_4()
            .rounded_md()
            .border_1()
            .border_color(variant.border(theme))
            .bg(variant.background(theme))
            .text_color(variant.foreground(theme))
            .text_sm()
            .child(self.label);

        if disabled {
            return button.opacity(0.5);
        }

        button = button.cursor_pointer().hover({
            let hover_background = variant.hover_background(theme);
            move |style| style.bg(hover_background)
        });

        match self.on_click {
            Some(on_click) => button.on_click(on_click),
            None => button,
        }
    }
}

/// A raised container with an optional heading.
#[derive(IntoElement)]
pub struct Card {
    children: Vec<AnyElement>,
    title: Option<SharedString>,
}

impl Card {
    /// An empty card.
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            title: None,
        }
    }

    /// Sets the heading shown above the card body.
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }
}

impl Default for Card {
    fn default() -> Self {
        Self::new()
    }
}

impl ParentElement for Card {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Card {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::global(cx);

        div()
            .flex()
            .flex_col()
            .gap_4()
            .p_6()
            .rounded_lg()
            .border_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .shadow_md()
            .when_some(self.title, |card, title| {
                card.child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.text_muted)
                        .child(title),
                )
            })
            .children(self.children)
    }
}

/// A one-pixel horizontal rule.
#[derive(IntoElement)]
pub struct Divider;

impl RenderOnce for Divider {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        div().h(px(1.)).w_full().bg(Theme::global(cx).border)
    }
}

/// Colours shared by every component.
///
/// Installed once at startup as a gpui global, so components never take a
/// palette parameter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    /// Page background.
    pub canvas: Hsla,
    /// Panel and card background.
    pub surface: Hsla,
    /// Panel background while hovered.
    pub surface_hover: Hsla,
    /// Hairline colour for borders and dividers.
    pub border: Hsla,
    /// Primary text.
    pub text: Hsla,
    /// Secondary text.
    pub text_muted: Hsla,
    /// Fill of primary actions.
    pub accent: Hsla,
    /// Fill of primary actions while hovered.
    pub accent_hover: Hsla,
    /// Text drawn on top of [`accent`](Self::accent).
    pub on_accent: Hsla,
}

impl Theme {
    /// The default dark palette.
    pub fn dark() -> Self {
        Self {
            canvas: rgb(0x0f0f12).into(),
            surface: rgb(0x1b1b1f).into(),
            surface_hover: rgb(0x232329).into(),
            border: rgb(0x2a2a31).into(),
            text: rgb(0xffffff).into(),
            text_muted: rgb(0x9a9aa5).into(),
            accent: rgb(0x2f6feb).into(),
            accent_hover: rgb(0x3d7bf5).into(),
            on_accent: rgb(0xffffff).into(),
        }
    }

    /// The active theme. Panics if the host installed none before rendering.
    pub fn global(cx: &App) -> &Theme {
        cx.global::<Theme>()
    }

    /// Makes this the active theme.
    pub fn install(self, cx: &mut App) {
        cx.set_global(self);
    }
}

impl Global for Theme {}
