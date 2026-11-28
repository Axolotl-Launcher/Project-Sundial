// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! A zoomable modal view of a decoded document image.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use axlauncher_ui::kit::Size;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, App, Bounds, Button, ButtonVariants, ClickEvent, Context, Corners, CursorStyle,
    DialogEntrance, Disableable, DragMoveEvent, IconName, IntoElement, MouseButton, MouseDownEvent,
    PinchEvent, Pixels, Point, Render, RenderImage, Role, ScrollEvent, SharedString, Sizable,
    Window, WindowExt, canvas, div, h_flex, point,
};

const MIN_ZOOM: f32 = 0.25;
const MAX_ZOOM: f32 = 8.;
const BUTTON_ZOOM_STEP: f32 = 1.25;
const WHEEL_ZOOM_STEP: f32 = 1.15;
const OPENING_HORIZONTAL_INSET_REMS: f32 = 4.;
const OPENING_VERTICAL_INSET_REMS: f32 = 8.;

pub(super) fn open(image: Arc<RenderImage>, name: SharedString, window: &mut Window, cx: &mut App) {
    let viewer = cx.new(|_| ImageViewer::new(image, name));
    window.open_dialog(cx, move |dialog, window, cx| {
        let viewport = window.viewport_size();
        let viewer = viewer.clone();
        dialog
            .centered()
            .entrance(DialogEntrance::Fade)
            .close_button(false)
            .min_h_0()
            .p_0()
            .border_0()
            .shadow_none()
            .bg(cx.theme().background.opacity(0.))
            .w(viewport.width)
            .h(viewport.height)
            .content(move |content, _, _| content.p_0().min_h_0().child(viewer.clone()))
    });
}

struct ImageViewer {
    image: Arc<RenderImage>,
    name: SharedString,
    intrinsic: Size<Pixels>,
    layout: Rc<Cell<ImageLayout>>,
    transform: ViewTransform,
    drag_origin: Option<(Point<Pixels>, Point<Pixels>)>,
}

impl ImageViewer {
    fn new(image: Arc<RenderImage>, name: SharedString) -> Self {
        let intrinsic = image
            .size(0)
            .map(|pixels| Pixels::from(i32::from(pixels) as f32));
        Self {
            image,
            name: name.trim().to_owned().into(),
            intrinsic,
            layout: Rc::default(),
            transform: ViewTransform::default(),
            drag_origin: None,
        }
    }

    fn zoom_at(&mut self, factor: f32, anchor: Point<Pixels>, cx: &mut Context<'_, Self>) {
        self.transform.zoom_at(factor, anchor, self.layout.get());
        cx.notify();
    }

    fn zoom_at_center(&mut self, factor: f32, cx: &mut Context<'_, Self>) {
        self.zoom_at(factor, self.layout.get().stage.center(), cx);
    }

    fn fit(&mut self, cx: &mut Context<'_, Self>) {
        self.transform = ViewTransform::fitted();
        cx.notify();
    }

    fn start_pan(&mut self, event: &MouseDownEvent, _: &mut Window, _: &mut Context<'_, Self>) {
        let layout = self.layout.get();
        let bounds = self.transform.bounds(layout);
        self.drag_origin = (self.transform.can_pan() && bounds.contains(&event.position))
            .then_some((event.position, bounds.center() - layout.stage.center()));
    }

    fn pan(&mut self, event: &DragMoveEvent<PanImage>, _: &mut Window, cx: &mut Context<'_, Self>) {
        if let Some((pointer, offset)) = self.drag_origin {
            self.transform
                .pan_to(offset + event.event.position - pointer, self.layout.get());
            cx.notify();
        }
    }

    fn click(&mut self, event: &ClickEvent, window: &mut Window, cx: &mut Context<'_, Self>) {
        let bounds = self.transform.bounds(self.layout.get());
        if event
            .mouse_position()
            .is_some_and(|position| !bounds.contains(&position))
        {
            window.close_dialog(cx);
        }
    }

    fn toolbar(&self, cx: &Context<'_, Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let layout = self.layout.get();
        let zoom = self.transform.level(layout);
        h_flex()
            .id("image-preview-toolbar")
            .occlude()
            .absolute()
            .top_2()
            .right_2()
            .gap_1()
            .p_1()
            .rounded(theme.radius_lg)
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .child(
                toolbar_button("zoom-out", IconName::Minus, "Zoom out")
                    .disabled(zoom <= layout.minimum_zoom())
                    .on_click(
                        cx.listener(|view, _, _, cx| {
                            view.zoom_at_center(1. / BUTTON_ZOOM_STEP, cx)
                        }),
                    ),
            )
            .child(
                Button::new("fit-image")
                    .small()
                    .ghost()
                    .label("Fit")
                    .tooltip("Fit image to window")
                    .on_click(cx.listener(|view, _, _, cx| view.fit(cx))),
            )
            .child(
                toolbar_button("zoom-in", IconName::Plus, "Zoom in")
                    .disabled(zoom >= MAX_ZOOM)
                    .on_click(
                        cx.listener(|view, _, _, cx| view.zoom_at_center(BUTTON_ZOOM_STEP, cx)),
                    ),
            )
            .child(
                toolbar_button(
                    "close-image-preview",
                    IconName::Close,
                    "Close image preview",
                )
                .on_click(|_, window, cx| window.close_dialog(cx)),
            )
    }

    fn caption(&self, cx: &App) -> impl IntoElement + use<> {
        let theme = cx.theme();
        h_flex()
            .absolute()
            .bottom_2()
            .left_2()
            .right_2()
            .justify_center()
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .rounded(theme.radius)
                    .bg(theme.popover)
                    .text_color(theme.popover_foreground)
                    .text_sm()
                    .px_2()
                    .py_1()
                    .child(self.name.clone()),
            )
    }

    fn image_canvas(&self) -> impl IntoElement + use<> {
        let layout = self.layout.clone();
        let transform = self.transform;
        let intrinsic = self.intrinsic;
        let image = self.image.clone();
        canvas(
            move |bounds, window, _| {
                let image_layout =
                    ImageLayout::new(intrinsic, bounds, window.viewport_size(), window.rem_size());
                layout.set(image_layout);
                transform.bounds(image_layout)
            },
            move |bounds, image_bounds, window, _| {
                let _ =
                    window.paint_image(bounds, image_bounds, Corners::default(), image, 0, false);
            },
        )
        .size_full()
    }

    fn stage(&self, cx: &mut Context<'_, Self>) -> impl IntoElement + use<> {
        div()
            .id("markdown-image-preview")
            .absolute()
            .inset_0()
            .role(Role::Image)
            .aria_label(if self.name.is_empty() {
                SharedString::from("Image preview")
            } else {
                self.name.clone()
            })
            .when(self.transform.can_pan(), |stage| {
                stage.cursor(CursorStyle::OpenHand)
            })
            .on_click(cx.listener(Self::click))
            .on_scroll(cx.listener(|view, event: &ScrollEvent, window, cx| {
                let delta = event.delta.pixel_delta(window.rem_size()).y / window.rem_size();
                view.zoom_at(WHEEL_ZOOM_STEP.powf(delta), event.position, cx);
                cx.stop_propagation();
            }))
            .on_pinch(cx.listener(|view, event: &PinchEvent, _, cx| {
                view.zoom_at(1. + event.delta, event.position, cx);
                cx.stop_propagation();
            }))
            .when(self.transform.can_pan(), |stage| {
                stage
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::start_pan))
                    .on_drag(PanImage, |_, _, _, cx| cx.new(|_| PanImage))
                    .on_drag_move(cx.listener(Self::pan))
            })
            .child(self.image_canvas())
    }
}

impl Render for ImageViewer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        div()
            .relative()
            .size_full()
            .overflow_hidden()
            .child(self.stage(cx))
            .when(!self.name.is_empty(), |view| view.child(self.caption(cx)))
            .child(self.toolbar(cx))
    }
}

struct PanImage;

impl Render for PanImage {
    fn render(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
        div().size_0()
    }
}

fn toolbar_button(id: &'static str, icon: IconName, label: &'static str) -> Button {
    Button::new(id)
        .small()
        .ghost()
        .icon(icon)
        .accessibility_label(label)
        .tooltip(label)
}

#[derive(Clone, Copy, Default)]
struct ViewTransform {
    zoom: Option<f32>,
    offset: Point<Pixels>,
}

impl ViewTransform {
    fn fitted() -> Self {
        Self {
            zoom: Some(1.),
            offset: Point::default(),
        }
    }

    fn level(self, layout: ImageLayout) -> f32 {
        self.zoom.unwrap_or_else(|| layout.opening_zoom())
    }

    fn can_pan(self) -> bool {
        self.zoom.is_some_and(|zoom| zoom > 1.)
    }

    fn zoom_at(&mut self, factor: f32, anchor: Point<Pixels>, layout: ImageLayout) {
        let current_zoom = self.level(layout);
        let zoom = (current_zoom * factor).clamp(layout.minimum_zoom(), MAX_ZOOM);
        let image_center = self.bounds(layout).center();
        let offset =
            anchor + (image_center - anchor) * (zoom / current_zoom) - layout.stage.center();
        self.zoom = Some(zoom);
        self.pan_to(offset, layout);
    }

    fn pan_to(&mut self, offset: Point<Pixels>, layout: ImageLayout) {
        self.offset = clamp_offset(offset, self.image_size(layout), layout.stage.size);
    }

    fn image_size(self, layout: ImageLayout) -> Size<Pixels> {
        if let Some(zoom) = self.zoom {
            return layout.fitted.map(|side| side * zoom);
        }
        layout.opening
    }

    fn bounds(self, layout: ImageLayout) -> Bounds<Pixels> {
        let image = self.image_size(layout);
        let offset = clamp_offset(self.offset, image, layout.stage.size);
        Bounds::centered_at(layout.stage.center() + offset, image)
    }
}

#[derive(Clone, Copy, Default)]
struct ImageLayout {
    stage: Bounds<Pixels>,
    fitted: Size<Pixels>,
    opening: Size<Pixels>,
}

impl ImageLayout {
    fn new(
        intrinsic: Size<Pixels>,
        stage: Bounds<Pixels>,
        viewport: Size<Pixels>,
        rem: Pixels,
    ) -> Self {
        let opening_area = Size {
            width: (viewport.width - rem * OPENING_HORIZONTAL_INSET_REMS).min(stage.size.width),
            height: (viewport.height - rem * OPENING_VERTICAL_INSET_REMS).min(stage.size.height),
        };
        let opening_limit = Size {
            width: opening_area.width.min(intrinsic.width),
            height: opening_area.height.min(intrinsic.height),
        };
        Self {
            stage,
            opening: fit_image(intrinsic, opening_limit),
            fitted: fit_image(intrinsic, stage.size),
        }
    }

    fn opening_zoom(self) -> f32 {
        if self.opening.width > Pixels::ZERO && self.fitted.width > Pixels::ZERO {
            return self.opening.width / self.fitted.width;
        }
        1.
    }

    fn minimum_zoom(self) -> f32 {
        MIN_ZOOM * self.opening_zoom().min(1.)
    }
}

fn clamp_offset(
    offset: Point<Pixels>,
    image: Size<Pixels>,
    viewport: Size<Pixels>,
) -> Point<Pixels> {
    let overflow = (image - viewport).map(|side| side.max(Pixels::ZERO) / 2.);
    point(
        offset.x.clamp(-overflow.width, overflow.width),
        offset.y.clamp(-overflow.height, overflow.height),
    )
}

fn fit_image(intrinsic: Size<Pixels>, available: Size<Pixels>) -> Size<Pixels> {
    let scale = (available.width / intrinsic.width)
        .min(available.height / intrinsic.height)
        .max(0.);
    intrinsic.map(|side| side * scale)
}

#[cfg(test)]
#[path = "../../../../tests/apps/ui/content-render/src/image_preview.rs"]
mod tests;
