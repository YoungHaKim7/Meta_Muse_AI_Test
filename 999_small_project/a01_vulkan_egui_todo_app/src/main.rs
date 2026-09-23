mod todo;

use std::sync::Arc;
use todo::TodoList;

use directories::ProjectDirs;
use smallvec::smallvec;
use vulkano::{
    VulkanLibrary,
    buffer::{Buffer, BufferContents, BufferCreateInfo, BufferUsage},
    command_buffer::{
        AutoCommandBufferBuilder, CommandBufferUsage, RenderPassBeginInfo,
        allocator::{StandardCommandBufferAllocator, StandardCommandBufferAllocatorCreateInfo},
    },
    device::{
        Device, DeviceCreateInfo, DeviceExtensions, Queue, QueueCreateInfo, QueueFlags,
        physical::PhysicalDeviceType,
    },
    image::{ImageUsage, view::ImageView},
    instance::{Instance, InstanceCreateFlags, InstanceCreateInfo, InstanceExtensions},
    memory::allocator::{AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator},
    pipeline::{
        GraphicsPipeline, PipelineLayout, PipelineShaderStageCreateInfo,
        graphics::{
            GraphicsPipelineCreateInfo,
            color_blend::{ColorBlendAttachmentState, ColorBlendState},
            input_assembly::InputAssemblyState,
            multisample::MultisampleState,
            rasterization::RasterizationState,
            vertex_input::{Vertex, VertexDefinition},
            viewport::{Viewport, ViewportState},
        },
        layout::PipelineDescriptorSetLayoutCreateInfo,
    },
    render_pass::{Framebuffer, FramebufferCreateInfo, RenderPass, Subpass},
    swapchain::{
        Surface, Swapchain, SwapchainCreateInfo, SwapchainPresentInfo, acquire_next_image,
    },
    sync::{self, GpuFuture},
};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{Key, NamedKey},
    window::{Window, WindowId},
};

#[derive(BufferContents, Vertex, Clone, Copy)]
#[repr(C)]
struct MyVertex {
    #[format(R32G32_SFLOAT)]
    position: [f32; 2],
    #[format(R32G32B32_SFLOAT)]
    color: [f32; 3],
}

mod vs {
    vulkano_shaders::shader! {
        ty: "vertex",
        src: r"
            #version 450
            layout(location = 0) in vec2 position;
            layout(location = 1) in vec3 color;
            layout(location = 0) out vec3 out_color;
            void main() {
                gl_Position = vec4(position, 0.0, 1.0);
                out_color = color;
            }
        ",
    }
}
mod fs {
    vulkano_shaders::shader! {
        ty: "fragment",
        src: r"
            #version 450
            layout(location = 0) in vec3 in_color;
            layout(location = 0) out vec4 f_color;
            void main() {
                f_color = vec4(in_color, 1.0);
            }
        ",
    }
}

struct App {
    instance: Option<Arc<Instance>>,
    device: Option<Arc<Device>>,
    queue: Option<Arc<Queue>>,
    surface: Option<Arc<Surface>>,
    swapchain: Option<Arc<Swapchain>>,
    swapchain_images: Option<Vec<Arc<vulkano::image::Image>>>,
    render_pass: Option<Arc<RenderPass>>,
    framebuffers: Vec<Arc<Framebuffer>>,
    pipeline: Option<Arc<GraphicsPipeline>>,
    command_buffer_allocator: Option<Arc<StandardCommandBufferAllocator>>,
    memory_allocator: Option<Arc<StandardMemoryAllocator>>,
    recreate_swapchain: bool,
    previous_frame_end: Option<Box<dyn GpuFuture>>,
    todo_list: TodoList,
    input_text: String,
    selected: usize,
    data_path: std::path::PathBuf,
    window: Option<Arc<Window>>,
}

impl App {
    fn new() -> Self {
        let data_path = if let Some(proj) = ProjectDirs::from("com", "example", "vulkan-todo") {
            let dir = proj.data_local_dir();
            std::fs::create_dir_all(dir).ok();
            dir.join("todos.json")
        } else {
            std::path::PathBuf::from("todos.json")
        };
        let todo_list = TodoList::load_from_file(&data_path);

        Self {
            instance: None,
            device: None,
            queue: None,
            surface: None,
            swapchain: None,
            swapchain_images: None,
            render_pass: None,
            framebuffers: Vec::new(),
            pipeline: None,
            command_buffer_allocator: None,
            memory_allocator: None,
            recreate_swapchain: false,
            previous_frame_end: None,
            todo_list,
            input_text: String::new(),
            selected: 0,
            data_path,
            window: None,
        }
    }

    fn init_vulkan(&mut self, event_loop: &ActiveEventLoop) {
        // macOS MoltenVK fix: get required extensions from winit
        let library = VulkanLibrary::new().expect("Failed to load Vulkan library");

        let required_extensions = Surface::required_extensions(event_loop).unwrap_or_default();

        // Check what the library actually supports
        let supported_extensions = library.supported_extensions();

        // Start with required extensions
        let mut enabled_extensions = required_extensions;
        // Add portability enumeration for MoltenVK if supported
        if supported_extensions.khr_portability_enumeration {
            enabled_extensions.khr_portability_enumeration = true;
        }
        if supported_extensions.ext_metal_surface {
            enabled_extensions.ext_metal_surface = true;
        }

        // Only keep extensions that are supported
        let enabled_extensions = enabled_extensions.intersection(&supported_extensions);

        println!("Enabling instance extensions: {:?}", enabled_extensions);

        let instance = Instance::new(
            &library,
            InstanceCreateInfo {
                flags: InstanceCreateFlags::ENUMERATE_PORTABILITY,
                enabled_extensions,
                ..Default::default()
            },
        )
        .expect("Failed to create Vulkan instance - try: brew install molten-vk");

        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title(Self::title(
                            &self.todo_list,
                            &self.input_text,
                            self.selected,
                        ))
                        .with_inner_size(winit::dpi::LogicalSize::new(900, 700)),
                )
                .unwrap(),
        );
        self.window = Some(window.clone());

        let surface = Surface::from_window(instance.clone(), window.clone())
            .expect("Failed to create surface - check MoltenVK installation");

        self.instance = Some(instance.clone());

        let device_extensions = DeviceExtensions {
            khr_swapchain: true,
            ..DeviceExtensions::empty()
        };

        let (physical_device, queue_family_index) = self
            .instance
            .as_ref()
            .unwrap()
            .enumerate_physical_devices()
            .unwrap()
            .filter(|p| p.supported_extensions().contains(&device_extensions))
            .filter_map(|p| {
                p.queue_family_properties()
                    .iter()
                    .enumerate()
                    .position(|(i, q)| {
                        q.queue_flags.intersects(QueueFlags::GRAPHICS)
                            && p.surface_support(i as u32, &surface).unwrap_or(false)
                    })
                    .map(|i| (p, i as u32))
            })
            .min_by_key(|(p, _)| match p.properties().device_type {
                PhysicalDeviceType::DiscreteGpu => 0,
                PhysicalDeviceType::IntegratedGpu => 1,
                _ => 2,
            })
            .expect("No suitable Vulkan device found");

        println!("Using device: {}", physical_device.properties().device_name);

        let (device, mut queues) = Device::new(
            physical_device,
            DeviceCreateInfo {
                queue_create_infos: vec![QueueCreateInfo {
                    queue_family_index,
                    ..Default::default()
                }],
                enabled_extensions: &device_extensions,
                ..Default::default()
            },
        )
        .unwrap();
        let queue = queues.next().unwrap();

        let memory_allocator = Arc::new(StandardMemoryAllocator::new_default(device.clone()));
        let command_buffer_allocator = Arc::new(StandardCommandBufferAllocator::new(
            device.clone(),
            StandardCommandBufferAllocatorCreateInfo::default(),
        ));

        let caps = device
            .physical_device()
            .surface_capabilities(&surface, Default::default())
            .unwrap();
        let format = device
            .physical_device()
            .surface_formats(&surface, Default::default())
            .unwrap()[0]
            .0;

        let (swapchain, images) = Swapchain::new(
            device.clone(),
            surface.clone(),
            SwapchainCreateInfo {
                min_image_count: caps.min_image_count.max(2),
                image_format: format,
                image_extent: window.inner_size().into(),
                image_usage: ImageUsage::COLOR_ATTACHMENT,
                composite_alpha: caps.supported_composite_alpha.into_iter().next().unwrap(),
                ..Default::default()
            },
        )
        .unwrap();

        let render_pass = vulkano::single_pass_renderpass!(
            device.clone(),
            attachments: {
                color: {
                    format: format,
                    samples: 1,
                    load_op: Clear,
                    store_op: Store,
                }
            },
            pass: {
                color: [color],
                depth_stencil: {}
            }
        )
        .unwrap();

        let vs = vs::load(device.clone()).unwrap();
        let fs = fs::load(device.clone()).unwrap();

        let pipeline = {
            let stages = [
                PipelineShaderStageCreateInfo::new(vs.entry_point("main").unwrap()),
                PipelineShaderStageCreateInfo::new(fs.entry_point("main").unwrap()),
            ];
            let layout = PipelineLayout::new(
                device.clone(),
                PipelineDescriptorSetLayoutCreateInfo::from_stages(&stages)
                    .into_pipeline_layout_create_info(device.clone())
                    .unwrap(),
            )
            .unwrap();
            let subpass = Subpass::from(render_pass.clone(), 0).unwrap();
            GraphicsPipeline::new(
                &device.clone(),
                None,
                GraphicsPipelineCreateInfo {
                    stages: stages.into_iter().collect(),
                    vertex_input_state: Some(
                        MyVertex::per_vertex()
                            .definition(&vs.entry_point("main").unwrap())
                            .unwrap(),
                    ),
                    input_assembly_state: Some(InputAssemblyState::default()),
                    viewport_state: Some(ViewportState::default()),
                    rasterization_state: Some(RasterizationState::default()),
                    multisample_state: Some(MultisampleState::default()),
                    color_blend_state: Some(ColorBlendState::with_attachment_states(
                        subpass.num_color_attachments(),
                        ColorBlendAttachmentState::default(),
                    )),
                    subpass: Some(subpass.into()),
                    ..GraphicsPipelineCreateInfo::layout(layout)
                },
            )
            .unwrap()
        };

        let framebuffers = images
            .iter()
            .map(|image| {
                let view = ImageView::new_default(image.clone()).unwrap();
                Framebuffer::new(
                    render_pass.clone(),
                    FramebufferCreateInfo {
                        attachments: vec![view],
                        ..Default::default()
                    },
                )
                .unwrap()
            })
            .collect();

        self.device = Some(device.clone());
        self.queue = Some(queue);
        self.surface = Some(surface);
        self.swapchain = Some(swapchain);
        self.swapchain_images = Some(images);
        self.render_pass = Some(render_pass);
        self.framebuffers = framebuffers;
        self.pipeline = Some(pipeline);
        self.memory_allocator = Some(memory_allocator);
        self.command_buffer_allocator = Some(command_buffer_allocator);
        self.previous_frame_end = Some(sync::now(device.clone()).boxed());
    }

    fn title(list: &TodoList, input: &str, selected: usize) -> String {
        let (total, done) = list.stats();
        let sel = list
            .items
            .get(selected)
            .map(|i| i.text.as_str())
            .unwrap_or("none");
        format!(
            "Vulkan Todo [{done}/{total}] Sel:{selected} '{}' | Input:'{}' | Enter=Add UpDown=Sel Space=Toggle Del=Remove",
            sel.chars().take(20).collect::<String>(),
            input.chars().take(25).collect::<String>()
        )
    }

    fn build_vertices(&self) -> Vec<MyVertex> {
        let mut verts = Vec::new();
        let n = self.todo_list.items.len().max(1) as f32;
        for (i, item) in self.todo_list.items.iter().enumerate() {
            let y_top = 0.9 - (i as f32 / n) * 1.8;
            let y_bottom = y_top - (1.4 / n);
            let (x_left, x_right) = (-0.9, 0.9);
            let base = if item.done {
                [0.2, 0.8, 0.3]
            } else {
                [0.9, 0.3, 0.3]
            };
            let color = if i == self.selected {
                [base[0] + 0.15, base[1] + 0.15, base[2] + 0.4]
            } else {
                base
            };
            verts.extend_from_slice(&[
                MyVertex {
                    position: [x_left, y_top],
                    color,
                },
                MyVertex {
                    position: [x_right, y_top],
                    color,
                },
                MyVertex {
                    position: [x_left, y_bottom],
                    color,
                },
                MyVertex {
                    position: [x_right, y_top],
                    color,
                },
                MyVertex {
                    position: [x_right, y_bottom],
                    color,
                },
                MyVertex {
                    position: [x_left, y_bottom],
                    color,
                },
            ]);
        }
        let w = (self.input_text.len() as f32 * 0.02).min(0.8);
        verts.extend_from_slice(&[
            MyVertex {
                position: [-0.9, -0.95],
                color: [0.3, 0.3, 0.9],
            },
            MyVertex {
                position: [-0.9 + w, -0.95],
                color: [0.3, 0.3, 0.9],
            },
            MyVertex {
                position: [-0.9, -0.85],
                color: [0.3, 0.3, 0.9],
            },
            MyVertex {
                position: [-0.9 + w, -0.95],
                color: [0.3, 0.3, 0.9],
            },
            MyVertex {
                position: [-0.9 + w, -0.85],
                color: [0.3, 0.3, 0.9],
            },
            MyVertex {
                position: [-0.9, -0.85],
                color: [0.3, 0.3, 0.9],
            },
        ]);
        verts
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.surface.is_none() {
            self.init_vulkan(event_loop);
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.todo_list.save_to_file(&self.data_path);
                event_loop.exit();
            }
            WindowEvent::Resized(_) => self.recreate_swapchain = true,
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key,
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                match logical_key {
                    Key::Named(NamedKey::Enter) => {
                        if !self.input_text.trim().is_empty() {
                            self.todo_list.add(self.input_text.clone());
                            self.input_text.clear();
                            self.todo_list.save_to_file(&self.data_path);
                        }
                    }
                    Key::Named(NamedKey::Backspace) => {
                        self.input_text.pop();
                    }
                    Key::Named(NamedKey::Delete) => {
                        self.todo_list.remove(self.selected);
                        if self.selected > 0 && self.selected >= self.todo_list.items.len() {
                            self.selected -= 1;
                        }
                        self.todo_list.save_to_file(&self.data_path);
                    }
                    Key::Named(NamedKey::ArrowUp) => {
                        if self.selected > 0 {
                            self.selected -= 1;
                        }
                    }
                    Key::Named(NamedKey::ArrowDown) => {
                        if self.selected + 1 < self.todo_list.items.len() {
                            self.selected += 1;
                        }
                    }
                    Key::Named(NamedKey::Space) => {
                        self.todo_list.toggle(self.selected);
                        self.todo_list.save_to_file(&self.data_path);
                    }
                    Key::Named(NamedKey::Escape) => self.input_text.clear(),
                    Key::Character(s) => self.input_text.push_str(&s),
                    _ => {}
                }
                if let Some(win) = &self.window {
                    win.set_title(&Self::title(
                        &self.todo_list,
                        &self.input_text,
                        self.selected,
                    ));
                }
                println!(
                    "TODO: {:?} | INPUT: '{}' | SEL:{}",
                    self.todo_list
                        .items
                        .iter()
                        .map(|i| format!("{}[{}]", i.text, if i.done { "x" } else { " " }))
                        .collect::<Vec<_>>(),
                    self.input_text,
                    self.selected
                );
            }
            WindowEvent::RedrawRequested => {
                if self.swapchain.is_none() {
                    return;
                }
                if self.recreate_swapchain {
                    let window = self.window.as_ref().unwrap();
                    let (new_swapchain, new_images) = self
                        .swapchain
                        .as_ref()
                        .unwrap()
                        .recreate(SwapchainCreateInfo {
                            image_extent: window.inner_size().into(),
                            ..self.swapchain.as_ref().unwrap().create_info()
                        })
                        .unwrap();
                    let render_pass = self.render_pass.as_ref().unwrap().clone();
                    let fbs = new_images
                        .iter()
                        .map(|img| {
                            let view = ImageView::new_default(&img.clone()).unwrap();
                            Framebuffer::new(
                                render_pass.clone(),
                                FramebufferCreateInfo {
                                    attachments: &[&view],
                                    ..Default::default()
                                },
                            )
                            .unwrap()
                        })
                        .collect();
                    self.swapchain = Some(new_swapchain);
                    self.swapchain_images = Some(new_images);
                    self.framebuffers = fbs;
                    self.recreate_swapchain = false;
                }
                let (image_i, suboptimal, acquire_future) =
                    match acquire_next_image(self.swapchain.clone().unwrap(), None) {
                        Ok(r) => r,
                        Err(vulkano::Validated::Error(vulkano::VulkanError::OutOfDate)) => {
                            self.recreate_swapchain = true;
                            return;
                        }
                        Err(e) => panic!("acquire: {e}"),
                    };
                if suboptimal {
                    self.recreate_swapchain = true;
                }
                let vertices = self.build_vertices();
                let vertex_buffer = Buffer::from_iter(
                    self.memory_allocator.as_ref().unwrap().clone(),
                    BufferCreateInfo {
                        usage: BufferUsage::VERTEX_BUFFER,
                        ..Default::default()
                    },
                    AllocationCreateInfo {
                        memory_type_filter: MemoryTypeFilter::PREFER_DEVICE
                            | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                        ..Default::default()
                    },
                    vertices,
                )
                .unwrap();
                let mut builder = AutoCommandBufferBuilder::primary(
                    self.command_buffer_allocator.as_ref().unwrap().clone(),
                    self.queue.as_ref().unwrap().queue_family_index(),
                    CommandBufferUsage::OneTimeSubmit,
                )
                .unwrap();
                let (_, done) = self.todo_list.stats();
                let ratio = if self.todo_list.items.is_empty() {
                    0.0
                } else {
                    done as f32 / self.todo_list.items.len() as f32
                };
                let clear = [0.05 * (1.0 - ratio), 0.05 + 0.15 * ratio, 0.12];
                builder
                    .begin_render_pass(
                        RenderPassBeginInfo {
                            clear_values: vec![Some(clear.into())],
                            ..RenderPassBeginInfo::framebuffer(
                                self.framebuffers[image_i as usize].clone(),
                            )
                        },
                        Default::default(),
                    )
                    .unwrap()
                    .set_viewport(
                        0,
                        smallvec![Viewport {
                            offset: [0.0, 0.0],
                            extent: self.window.as_ref().unwrap().inner_size().into(),
                            depth_range: 0.0..=1.0,
                        }],
                    )
                    .unwrap()
                    .bind_pipeline_graphics(self.pipeline.as_ref().unwrap().clone())
                    .unwrap()
                    .bind_vertex_buffers(0, vertex_buffer.clone())
                    .unwrap();
                unsafe { builder.draw(vertex_buffer.len() as u32, 1, 0, 0) }.unwrap();
                builder.end_render_pass(Default::default()).unwrap();
                let command_buffer = builder.build().unwrap();
                let future = self
                    .previous_frame_end
                    .take()
                    .unwrap()
                    .join(acquire_future)
                    .then_execute(self.queue.as_ref().unwrap().clone(), command_buffer)
                    .unwrap()
                    .then_swapchain_present(
                        self.queue.as_ref().unwrap().clone(),
                        SwapchainPresentInfo::swapchain_image_index(
                            self.swapchain.clone().unwrap(),
                            image_i,
                        ),
                    )
                    .then_signal_fence_and_flush();
                match future.map(|f| f.boxed()) {
                    Ok(f) => self.previous_frame_end = Some(f),
                    Err(vulkano::Validated::Error(vulkano::VulkanError::OutOfDate)) => {
                        self.recreate_swapchain = true;
                        self.previous_frame_end =
                            Some(sync::now(self.device.as_ref().unwrap().clone()).boxed());
                    }
                    Err(e) => {
                        eprintln!("flush: {e}");
                        self.previous_frame_end =
                            Some(sync::now(self.device.as_ref().unwrap().clone()).boxed());
                    }
                }
            }
            _ => {}
        }
        if let Some(win) = &self.window {
            win.request_redraw();
        }
    }
    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(win) = &self.window {
            win.request_redraw();
        }
    }
}

fn main() -> anyhow::Result<()> {
    let event_loop = EventLoop::new()?;
    let mut app = App::new();
    event_loop.run_app(&mut app)?;
    Ok(())
}
