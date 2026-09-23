mod todo;

use egui::{CentralPanel, Color32, Context, RichText, ScrollArea};
use egui_winit_vulkano::{Gui, GuiConfig};
use std::sync::Arc;
use vulkano::{
    swapchain::PresentMode,
    sync::{self, GpuFuture},
};
use vulkano_util::{
    context::{VulkanoConfig, VulkanoContext},
    window::{VulkanoWindows, WindowDescriptor},
};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};

use directories::ProjectDirs;
use todo::{Priority, TodoList};

struct App {
    context: VulkanoContext,
    windows: VulkanoWindows,
    gui: Option<Gui>,
    todo_list: TodoList,
    input_text: String,
    filter: Filter,
    data_path: std::path::PathBuf,
    recreate_swapchain: bool,
    previous_frame_end: Option<Box<dyn GpuFuture>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Filter {
    #[default]
    All,
    Active,
    Completed,
}

impl App {
    fn new() -> Self {
        // VulkanoContext::new takes VulkanoConfig, not Instance directly
        let context = VulkanoContext::new(VulkanoConfig::default());

        let windows = VulkanoWindows::default();

        let data_path = if let Some(proj) = ProjectDirs::from("com", "example", "vulkan-todo") {
            let dir = proj.data_local_dir();
            std::fs::create_dir_all(dir).ok();
            dir.join("todos.json")
        } else {
            std::path::PathBuf::from("todos.json")
        };

        let todo_list = TodoList::load_from_file(&data_path);

        Self {
            context,
            windows,
            gui: None,
            todo_list,
            input_text: String::new(),
            filter: Filter::All,
            data_path,
            recreate_swapchain: false,
            previous_frame_end: Some(sync::now(context.device().clone()).boxed()),
        }
    }

    fn ensure_gui(&mut self, window_id: WindowId) {
        if self.gui.is_none() {
            let window = self.windows.get_window(window_id).unwrap();
            let renderer = self.windows.get_renderer(window_id).unwrap();
            self.gui = Some(Gui::new(
                window,
                renderer.swapchain_format(),
                renderer.graphics_queue(),
                renderer.subpass(),
                GuiConfig {
                    is_overlay: false,
                    ..Default::default()
                },
            ));
        }
    }

    // egui 0.31 API: panels take &Context, not &mut Ui (for 0.36 it would be Panel::top)
    fn ui(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(RichText::new("⚡ Vulkan Todo").size(22.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (total, done) = self.todo_list.stats();
                    ui.label(format!("{done}/{total} done"));
                    if ui.button("Clear completed").clicked() {
                        self.todo_list.clear_completed();
                        self.todo_list.save_to_file(&self.data_path);
                    }
                });
            });
        });

        egui::TopBottomPanel::bottom("input").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.input_text)
                        .hint_text("What needs to be done? (Vulkan-powered)")
                        .desired_width(ui.available_width() - 110.0),
                );
                let add_clicked = ui
                    .add_sized([100.0, 28.0], egui::Button::new("Add + Enter"))
                    .clicked()
                    || (response.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        && !self.input_text.trim().is_empty());

                if add_clicked {
                    self.todo_list.add(self.input_text.clone());
                    self.input_text.clear();
                    self.todo_list.save_to_file(&self.data_path);
                }
            });
            ui.add_space(6.0);
        });

        CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.filter, Filter::All, "All");
                ui.selectable_value(&mut self.filter, Filter::Active, "Active");
                ui.selectable_value(&mut self.filter, Filter::Completed, "Completed");
                ui.separator();
                ui.label(
                    RichText::new(format!(
                        "GPU: {}",
                        self.context
                            .device()
                            .physical_device()
                            .properties()
                            .device_name
                    ))
                    .weak()
                    .size(11.0),
                );
            });
            ui.separator();

            ScrollArea::vertical().show(ui, |ui| {
                let mut to_toggle: Option<u64> = None;
                let mut to_remove: Option<u64> = None;

                let filtered: Vec<_> = self
                    .todo_list
                    .items
                    .iter()
                    .filter(|item| match self.filter {
                        Filter::All => true,
                        Filter::Active => !item.done,
                        Filter::Completed => item.done,
                    })
                    .cloned()
                    .collect();

                if filtered.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(40.0);
                        ui.label(
                            RichText::new("No tasks here. Add one below!")
                                .size(16.0)
                                .weak(),
                        );
                    });
                }

                for item in filtered {
                    ui.horizontal(|ui| {
                        let mut checked = item.done;
                        if ui.checkbox(&mut checked, "").changed() {
                            to_toggle = Some(item.id);
                        }
                        let col = match item.priority {
                            Priority::High => Color32::RED,
                            Priority::Medium => Color32::GOLD,
                            Priority::Low => Color32::LIGHT_GREEN,
                        };
                        ui.colored_label(col, "●");
                        let mut text = RichText::new(&item.text).size(15.0);
                        if item.done {
                            text = text.strikethrough().weak();
                        }
                        ui.label(text);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("🗑").clicked() {
                                to_remove = Some(item.id);
                            }
                            ui.label(
                                RichText::new(item.created_at.format("%H:%M").to_string())
                                    .weak()
                                    .size(11.0),
                            );
                        });
                    });
                    ui.separator();
                }

                if let Some(id) = to_toggle {
                    self.todo_list.toggle(id);
                    self.todo_list.save_to_file(&self.data_path);
                }
                if let Some(id) = to_remove {
                    self.todo_list.remove(id);
                    self.todo_list.save_to_file(&self.data_path);
                }
            });
        });
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // vulkano-util 0.35 API: create_window(event_loop, context, descriptor, swapchain_config_fn)
        let _id = self.windows.create_window(
            event_loop,
            self.context.clone(),
            &WindowDescriptor {
                title: "Vulkan Todo - Rust + Vulkan".to_string(),
                width: 900.0,
                height: 700.0,
                present_mode: PresentMode::Fifo,
                ..Default::default()
            },
            |_| {},
        );
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if let Some(gui) = self.gui.as_mut() {
            let _consumed = gui.update(&event);
        }

        match event {
            WindowEvent::CloseRequested => {
                self.todo_list.save_to_file(&self.data_path);
                event_loop.exit();
            }
            WindowEvent::Resized(_) => {
                self.recreate_swapchain = true;
            }
            WindowEvent::RedrawRequested => {
                self.ensure_gui(window_id);
                let renderer = self.windows.get_renderer_mut(window_id).unwrap();

                if self.recreate_swapchain {
                    renderer.resize();
                    self.recreate_swapchain = false;
                }

                let gui = self.gui.as_mut().unwrap();
                gui.immediate_ui(|gui| {
                    let ctx = gui.context().clone();
                    self.ui(&ctx);
                });

                let before = self.previous_frame_end.take().unwrap();
                let after = renderer.acquire(None, |_| {}).unwrap();
                let cb = renderer.draw(None, |builder| {
                    gui.draw(builder);
                });

                let future = before
                    .join(after)
                    .then_execute(renderer.graphics_queue(), cb)
                    .unwrap()
                    .then_swapchain_present(
                        renderer.graphics_queue(),
                        vulkano::swapchain::SwapchainPresentInfo::swapchain_image_index(
                            renderer.swapchain(),
                            renderer.image_index(),
                        ),
                    )
                    .then_signal_fence_and_flush();

                match future {
                    Ok(f) => self.previous_frame_end = Some(f.boxed()),
                    Err(vulkano::Validated::Error(vulkano::VulkanError::OutOfDate)) => {
                        self.recreate_swapchain = true;
                        self.previous_frame_end =
                            Some(sync::now(self.context.device().clone()).boxed());
                    }
                    Err(e) => {
                        eprintln!("Failed to flush future: {e}");
                        self.previous_frame_end =
                            Some(sync::now(self.context.device().clone()).boxed());
                    }
                }
            }
            _ => {}
        }

        if let Some(window) = self.windows.get_window(window_id) {
            window.request_redraw();
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        for (_, renderer) in self.windows.iter() {
            renderer.window().request_redraw();
        }
    }
}

fn main() -> anyhow::Result<()> {
    let event_loop = EventLoop::new().unwrap();
    let mut app = App::new();
    event_loop.run_app(&mut app)?;
    Ok(())
}
