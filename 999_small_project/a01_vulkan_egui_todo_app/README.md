# Vulkan Todo App – Rust

A fully functional Todo app that renders its UI **with Vulkan** (not just using a window toolkit).

Stack:
- **Vulkan**: via `vulkano 0.35` (safe Rust wrapper)
- **Window**: `winit 0.30`
- **UI**: `egui 0.31` rendered by `egui_winit_vulkano`
- **Persistence**: JSON file in OS data dir (`directories` crate)

This is overkill for a Todo app – and that's the point. You get:
- Real Vulkan instance/device/swapchain creation
- Proper swapchain recreation on resize
- egui integrated as a Vulkan subpass
- GPU future chaining

## Run

Prerequisites: Vulkan SDK / drivers installed. On macOS: `brew install molten-vk vulkan-loader`.

```bash
cargo run --release
```

## Features

- Add / toggle / delete todos
- Filter: All / Active / Completed
- Clear completed
- Auto-save to `todos.json` in data dir
- Shows GPU name in header (proves Vulkan is used)

## Project layout

```
src/
  main.rs  – Vulkan setup, winit event loop, egui integration, render loop
  todo.rs  – Todo data model, CRUD, serde persistence
Cargo.toml
```

# Install Dependencies

```bash
brew install molten-vk vulkan-loader
```

## Why Vulkan for a Todo?

1. Learn Vulkan without writing 2000 lines of raw ash code
2. vulkano gives you safe abstractions, egui gives you instant UI
3. You can extend this to 3D todo visualizations, particle effects on completion, etc.

## Extending

- Add priorities: edit `todo.rs` Priority handling in UI
- Add search: filter `self.todo_list.items` by text
- Add custom Vulkan background: add a first subpass that draws a gradient triangle before egui subpass

License: MIT



