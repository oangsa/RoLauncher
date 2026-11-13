//! RoLauncher GPUI desktop entrypoint.

#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

mod api;
mod app;
mod bootstrap;
mod components;
mod dialogs;
mod domain;
mod pages;
mod platform;
#[cfg(feature = "ui-smoke")]
mod smoke;
mod state;
mod theme;
mod updater;

use crate::api::ApiClient;
use crate::app::{RoLauncherApp, bind_shell_keys};
use crate::bootstrap::Bootstrap;
use crate::components::text_input::bind_text_input_keys;
use crate::platform::process::show_fatal_message_box;
use crate::platform::window::initial_window_bounds;
use gpui::{App, Application, TitlebarOptions, WindowOptions, prelude::*};
use std::rc::Rc;

fn main() {
    let bootstrap = match Bootstrap::read_from_stdin() {
        Some(b) => b,
        None => {
            show_fatal_message_box(
                "RoLauncher Desktop Startup Error",
                "Missing or invalid startup bootstrap JSON on stdin.",
            );
            std::process::exit(1);
        }
    };

    let api = match ApiClient::new(bootstrap.port, bootstrap.token.to_string()) {
        Ok(c) => c,
        Err(err) => {
            show_fatal_message_box("RoLauncher Desktop Startup Error", &err);
            std::process::exit(1);
        }
    };

    let platform = match gpui_windows::WindowsPlatform::new(false) {
        Ok(p) => p,
        Err(err) => {
            show_fatal_message_box(
                "RoLauncher Graphics Initialization Failed",
                &format!("Unable to initialize Windows GPU interface: {err}"),
            );
            std::process::exit(1);
        }
    };

    let version_title = format!("RoLauncher {}", bootstrap.version);
    Application::with_platform(Rc::new(platform)).run(move |cx: &mut App| {
        bind_text_input_keys(cx);
        bind_shell_keys(cx);

        cx.on_window_closed(|cx, _id| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let (bounds, min_size) = initial_window_bounds(cx);
        if let Err(err) = cx.open_window(
            WindowOptions {
                window_bounds: Some(bounds),
                window_min_size: Some(min_size),
                titlebar: Some(TitlebarOptions {
                    title: Some(version_title.into()),
                    appears_transparent: false,
                    traffic_light_position: None,
                }),
                ..Default::default()
            },
            move |window, cx| cx.new(|cx| RoLauncherApp::new(bootstrap, api, window, cx)),
        ) {
            show_fatal_message_box(
                "RoLauncher Window Error",
                &format!("Unable to open RoLauncher window: {err}"),
            );
            cx.quit();
        }
    });
}
