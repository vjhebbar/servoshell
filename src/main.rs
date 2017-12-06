/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#![feature(box_syntax)]
#![feature(link_args)]
#![feature(slice_patterns)]

#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate serde;
extern crate treediff;

#[macro_use]
extern crate log;

#[cfg(all(not(feature = "force-glutin"), target_os = "macos"))]
extern crate libc;
#[cfg(all(not(feature = "force-glutin"), target_os = "macos"))]
extern crate cocoa;
#[cfg(all(not(feature = "force-glutin"), target_os = "macos"))]
#[macro_use]
extern crate objc;

#[cfg(any(feature = "force-glutin", not(target_os = "macos")))]
extern crate glutin;
#[cfg(any(feature = "force-glutin", not(target_os = "macos")))]
extern crate tinyfiledialogs;


#[cfg(target_os = "windows")]
extern crate winapi;
#[cfg(target_os = "windows")]
extern crate user32;
#[cfg(target_os = "windows")]
extern crate gdi32;

extern crate open;

mod traits;
mod platform;
mod servo;
mod state;
mod logs;

use platform::App;
use servo::{Servo, ServoEvent, ServoUrl, WebRenderDebugOption};
use state::{AppState, State, WindowState};
use std::env;
use std::env::args;
use std::error::Error;
use std::io::prelude::*;
use std::fs::File;
use std::path::Path;
use std::rc::Rc;
use traits::app::{AppEvent, AppCommand, AppMethods};
use traits::view::*;
use traits::window::{WindowEvent, WindowCommand};

const PKG_VERSION: &'static str = env!("CARGO_PKG_VERSION");
const PKG_NAME: &'static str = env!("CARGO_PKG_NAME");

fn main() {

    let logs = logs::Logger::init();

    info!("starting");

    args()
        .find(|arg| arg == "--version")
        .map(|_| {
                 println!("{} {}", PKG_NAME, PKG_VERSION);
                 std::process::exit(0);
             });

    let resources_path = App::get_resources_path().expect("Can't find resources path");

    let mut app_state = State::new(AppState::new());
    app_state.get_mut().current_window_index = Some(0);

    let mut win_state = State::new(WindowState::new());

    let app = App::new(app_state.get()).expect("Can't create application");
    let win = app.new_window(win_state.get())
        .expect("Can't create application");
    app_state.snapshot();
    win_state.snapshot();

    let view = win.new_view().unwrap();

    Servo::configure(resources_path.clone());

    let servo = {
        let geometry = view.get_geometry();
        let waker = win.new_event_loop_waker();
        Servo::new(geometry, view.clone(), waker)
    };

    let home_url = resources_path
        .parent()
        .unwrap()
        .join("shell_resources")
        .join("home.html");
    let home_url = ServoUrl::from_file_path(&home_url)
        .unwrap()
        .into_string();

    // Skip first argument (executable), and find the first
    // argument that doesn't start with `-`
    let url = args()
        .skip(1)
        .find(|arg| !arg.starts_with("-"))
        .unwrap_or(home_url);

    let browser = servo.new_browser(&url);
    servo.select_browser(browser.id);

    win_state
        .get_mut()
        .tabs
        .append_new(browser)
        .expect("Can't append browser");
    win.render(win_state.diff(), win_state.get());
    win_state.snapshot();

    info!("Servo version: {}", servo.version());

    let handle_events = || {

        // Loop until no events are available anymore.
        loop {

            let app_events = app.get_events();
            let win_events = win.get_events();
            let view_events = view.get_events();
            let servo_events = servo.get_events();

            if app_events.is_empty() && win_events.is_empty() && view_events.is_empty() &&
               servo_events.is_empty() {
                break;
            }

            // FIXME: it's really annoying we need this
            let mut force_sync = false;

            for event in win_events {
                if handle_win_event(&servo, &view, &mut win_state, &mut app_state, event)
                           .expect("handle_win_event exception") {
                        force_sync = true;
                    }
            }

            for event in app_events {
                handle_app_event(&servo,
                                 &view,
                                 &mut win_state,
                                 &mut app_state,
                                 event).expect("handle_app_event exception");
            }

            for event in view_events {
                handle_view_event(&servo, &view, &mut win_state, &mut app_state, event)
                        .expect("handle_view_event exception");
            }

            for event in servo_events {
                handle_servo_event(&servo, &view, &mut win_state, &mut app_state, event)
                        .expect("handle_servo_event exception");
            }

            if app_state.has_changed() || win_state.has_changed() {
                app.render(app_state.diff(), app_state.get());
                win.render(win_state.diff(), win_state.get());
                app_state.snapshot();
                win_state.snapshot();
            }

            servo.sync(force_sync);
        }

        // Here, only stuff that we know for sure won't trigger any
        // new events

        // FIXME: logs will grow until pulled
        if win_state.get().logs_visible {
            win.append_logs(&logs.get_logs());
        }
    };

    view.set_live_resize_callback(&handle_events);

    app.run(handle_events);

}

fn handle_win_event(servo: &Servo,
                    view: &Rc<ViewMethods>,
                    win_state: &mut State<WindowState>,
                    _app_state: &mut State<AppState>,
                    event: WindowEvent)
                    -> Result<bool, &'static str> {

    match event {
        WindowEvent::EventLoopAwaken => {
            return Ok(true);
        }
        WindowEvent::GeometryDidChange => {
            servo.update_geometry(view.get_geometry());
            view.update_drawable();
        }
        WindowEvent::DidEnterFullScreen => {
            // FIXME
        }
        WindowEvent::DidExitFullScreen => {
            // FIXME
        }
        WindowEvent::WillClose => {
            // FIXME
        }
        WindowEvent::OptionsClosed => {
            win_state.get_mut().options_open = false;
        }
        WindowEvent::UrlbarFocusChanged(focused) => {
            win_state
                .get_mut()
                .tabs
                .mut_fg_browser()?
                .urlbar_focused = focused;
        }
        WindowEvent::DoCommand(cmd) => {
            let bid = win_state.get().tabs.ref_fg_browser()?.id;
            match cmd {
                WindowCommand::Stop => {
                    // FIXME
                }
                WindowCommand::Reload => {
                    servo.reload(bid);
                }
                WindowCommand::NavigateBack => {
                    servo.go_back(bid);
                }
                WindowCommand::NavigateForward => {
                    servo.go_forward(bid);
                }
                WindowCommand::OpenLocation => {
                    win_state
                        .get_mut()
                        .tabs
                        .mut_fg_browser()?
                        .urlbar_focused = true;
                }
                WindowCommand::OpenInDefaultBrowser => {
                    if let Some(ref url) = win_state.get().tabs.ref_fg_browser()?.url {
                        open::that(url.clone()).ok();
                    }
                }
                WindowCommand::ZoomIn => {
                    win_state.get_mut().tabs.mut_fg_browser()?.zoom *= 1.1;
                    servo.zoom(win_state.get().tabs.ref_fg_browser()?.zoom);
                }
                WindowCommand::ZoomOut => {
                    win_state.get_mut().tabs.mut_fg_browser()?.zoom /= 1.1;
                    servo.zoom(win_state.get().tabs.ref_fg_browser()?.zoom);
                }
                WindowCommand::ZoomToActualSize => {
                    win_state.get_mut().tabs.mut_fg_browser()?.zoom = 1.0;
                    servo.reset_zoom();
                }

                WindowCommand::ToggleSidebar => {
                    win_state.get_mut().sidebar_is_open = !win_state.get().sidebar_is_open;
                }

                WindowCommand::ShowOptions => {
                    win_state.get_mut().options_open = !win_state.get().options_open;
                }

                WindowCommand::Load(request) => {
                    win_state.get_mut().tabs.mut_fg_browser()?.user_input = Some(request.clone());
                    win_state
                        .get_mut()
                        .tabs
                        .mut_fg_browser()?
                        .urlbar_focused = false;
                    let url = ServoUrl::parse(&request)
                        .or_else(|error| {
                            // See: https://github.com/paulrouget/servoshell/issues/59
                            if request.ends_with(".com") || request.ends_with(".org") ||
                               request.ends_with(".net") {
                                ServoUrl::parse(&format!("http://{}", request))
                            } else {
                                Err(error)
                            }
                        })
                        .or_else(|_| {
                                     ServoUrl::parse(&format!("https://duckduckgo.com/html/?q={}",
                                                              request))
                                 });
                    match url {
                        Ok(url) => servo.load_url(bid, url),
                        Err(err) => warn!("Can't parse url: {}", err),
                    }
                }
                WindowCommand::ToggleOptionShowLogs => {
                    win_state.get_mut().logs_visible = !win_state.get().logs_visible;
                }
                WindowCommand::NewTab => {
                    let mut browser = servo.new_browser("about:blank");
                    browser.is_background = false;
                    if cfg!(all(not(feature = "force-glutin"), target_os = "macos")) {
                        browser.urlbar_focused = true;
                    }
                    win_state.get_mut().tabs.append_new(browser)?;
                    let new = win_state.get().tabs.ref_fg_browser()?.id;
                    servo.select_browser(new);
                    servo.update_geometry(view.get_geometry());
                }
                WindowCommand::CloseTab => {
                    if win_state.get().tabs.has_more_than_one() {
                        let old = win_state.get_mut().tabs.kill_fg()?;
                        servo.close_browser(old);
                        let new = win_state.get().tabs.ref_fg_browser()?.id;
                        servo.select_browser(new);
                    }
                }
                WindowCommand::PrevTab => {
                    if win_state.get().tabs.has_more_than_one() {
                        if win_state.get().tabs.can_select_prev().unwrap() {
                            win_state.get_mut().tabs.select_prev()?;
                        } else {
                            win_state.get_mut().tabs.select_last()?;
                        }
                        let new = win_state.get().tabs.ref_fg_browser()?.id;
                        servo.select_browser(new);
                    }
                }
                WindowCommand::NextTab => {
                    if win_state.get().tabs.has_more_than_one() {
                        if win_state.get().tabs.can_select_next().unwrap() {
                            win_state.get_mut().tabs.select_next()?;
                        } else {
                            win_state.get_mut().tabs.select_first()?;
                        }
                        let new = win_state.get().tabs.ref_fg_browser()?.id;
                        servo.select_browser(new);
                    }
                }
                WindowCommand::SelectTab(idx) => {
                    if win_state.get().tabs.can_select_nth(idx) {
                        win_state.get_mut().tabs.select_nth(idx)?;
                        let new = win_state.get().tabs.ref_fg_browser()?.id;
                        servo.select_browser(new);
                    }
                }
                WindowCommand::ToggleOptionFragmentBorders => {}
                WindowCommand::ToggleOptionParallelDisplayListBuidling => {}
                WindowCommand::ToggleOptionShowParallelLayout => {}
                WindowCommand::ToggleOptionConvertMouseToTouch => {}
                WindowCommand::ToggleOptionTileBorders => {}

                WindowCommand::ToggleOptionWRProfiler => {
                    win_state.get_mut().debug_options.wr_profiler =
                        !win_state.get().debug_options.wr_profiler;
                    servo.toggle_webrender_debug_option(WebRenderDebugOption::Profiler);
                }

                WindowCommand::ToggleOptionWRTextureCacheDebug => {
                    win_state.get_mut().debug_options.wr_texture_cache_debug =
                        !win_state.get().debug_options.wr_texture_cache_debug;
                    servo.toggle_webrender_debug_option(WebRenderDebugOption::TextureCacheDebug);
                }

                WindowCommand::ToggleOptionWRTargetDebug => {
                    win_state.get_mut().debug_options.wr_render_target_debug =
                        !win_state.get().debug_options.wr_render_target_debug;
                    servo.toggle_webrender_debug_option(WebRenderDebugOption::RenderTargetDebug);
                }
            }
        }
    }
    Ok(false)
}


fn handle_app_event(servo: &Servo,
                    view: &Rc<ViewMethods>,
                    _win_state: &mut State<WindowState>,
                    app_state: &mut State<AppState>,
                    event: AppEvent)
                    -> Result<(), &'static str> {

    match event {
        AppEvent::DidFinishLaunching => {
            // FIXME: does this work?
        }
        AppEvent::WillTerminate => {
            // FIXME: does this work?
        }
        AppEvent::DidChangeScreenParameters => {
            // FIXME: does this work?
            servo.update_geometry(view.get_geometry());
            view.update_drawable();
        }
        AppEvent::DoCommand(cmd) => {
            match cmd {
                AppCommand::ClearHistory => {
                    // FIXME
                }
                AppCommand::ToggleOptionDarkTheme => {
                    app_state.get_mut().dark_theme = !app_state.get().dark_theme;
                }
            }
        }
    };
    Ok(())
}



fn handle_view_event(servo: &Servo,
                     view: &Rc<ViewMethods>,
                     win_state: &mut State<WindowState>,
                     _app_state: &mut State<AppState>,
                     event: ViewEvent)
                     -> Result<(), &'static str> {

    match event {
        ViewEvent::GeometryDidChange => {
            servo.update_geometry(view.get_geometry());
            view.update_drawable();
        }
        ViewEvent::MouseWheel(delta, phase) => {
            // FIXME: magic value
            static LINE_HEIGHT: f32 = 38.0;
            let (mut x, mut y) = match delta {
                MouseScrollDelta::PixelDelta(x, y) => (x, y),
                MouseScrollDelta::LineDelta(x, y) => (x, y * LINE_HEIGHT),
            };
            if y.abs() >= x.abs() {
                x = 0.0;
            } else {
                y = 0.0;
            }
            servo.perform_scroll(0, 0, x, y, phase);
        }
        ViewEvent::MouseMoved(x, y) => {
            servo.perform_mouse_move(x, y);
        }
        ViewEvent::MouseInput(element_state, button, x, y) => {
            servo.perform_click(x, y, element_state, button);
        }
        ViewEvent::KeyEvent(c, key, keystate, modifiers) => {
            let id = win_state
                .get()
                .tabs
                .ref_fg_browser()
                .expect("no current browser")
                .id;
            servo.send_key(id, c, key, keystate, modifiers);
        }
    };
    Ok(())
}



fn handle_servo_event(_servo: &Servo,
                      view: &Rc<ViewMethods>,
                      win_state: &mut State<WindowState>,
                      app_state: &mut State<AppState>,
                      event: ServoEvent)
                      -> Result<(), &'static str> {

    match event {
        ServoEvent::SetWindowInnerSize(..) => {
            // ignore
        }
        ServoEvent::SetWindowPosition(..) => {
            // ignore
        }
        ServoEvent::SetFullScreenState(fullscreen) => {
            if fullscreen {
                view.enter_fullscreen();
            } else {
                view.exit_fullscreen();
            }
        }
        ServoEvent::TitleChanged(id, title) => {
            match win_state.get_mut().tabs.find_browser(&id) {
                Some(browser) => {
                    browser.title = title;
                }
                None => warn!("Got message for unkown browser:  {:?}", id),
            }
        }
        ServoEvent::StatusChanged(status) => {
            win_state.get_mut().status = status;
        }
        ServoEvent::LoadStart(id) => {
            match win_state.get_mut().tabs.find_browser(&id) {
                Some(browser) => {
                    browser.is_loading = true;
                }
                None => warn!("Got message for unkown browser:  {:?}", id),
            }
        }
        ServoEvent::LoadEnd(id) => {
            match win_state.get_mut().tabs.find_browser(&id) {
                Some(browser) => {
                    browser.is_loading = false;
                }
                None => warn!("Got message for unkown browser:  {:?}", id),
            }
        }
        ServoEvent::HeadParsed(..) => {
            // FIXME
        }
        ServoEvent::HistoryChanged(id, entries, current) => {
            match win_state.get_mut().tabs.find_browser(&id) {
                Some(browser) => {
                    let url = entries[current].url.to_string();
                    browser.url = Some(url);
                    browser.can_go_back = current > 0;
                    browser.can_go_forward = current < entries.len() - 1;
                }
                None => warn!("Got message for unkown browser:  {:?}", id),
            }
        }
        ServoEvent::CursorChanged(cursor) => {
            // FIXME: Work-around https://github.com/servo/servo/issues/18599
            // FIXME: also, see https://github.com/paulrouget/servoshell/issues/67
            if cursor != app_state.get().cursor {
                app_state.get_mut().cursor = cursor;
            }
        }
        ServoEvent::FaviconChanged(..) => {
            // FIXME
        }
        ServoEvent::Key(..) => {
            // FIXME
        }
        ServoEvent::OpenInDefaultBrowser(url) => {
            open::that(url).ok();
        }
        ServoEvent::WriteMicrodata(microdata, datatype) => {
            match env::home_dir() {
                Some(path) => {
                    let file_name = match datatype.as_str() {
                        "vcard" => {
                            format!("{}/microdata.vcf", path.to_str().unwrap())
                        },
                        "json" => {
                            format!("{}/microdata.json", path.to_str().unwrap())
                        },
                        _ => {
                            panic!("Microdata type not passed as argument");
                        },
                    };

                    let path = Path::new(&file_name);
                    let mut file = match File::create(&path) {
                        Err(why) => panic!("couldn't create: {}, {}",
                                           file_name,
                                           why.description()),
                        Ok(file) => file,
                    };
                    match file.write_all(microdata.as_bytes()) {
                        Err(why) => {
                            panic!("couldn't write to {}: {}", file_name,
                                                               why.description())
                        },
                        Ok(_) => {
                            println!("successfully wrote microdata to {}", file_name);
                            let id = win_state
                                .get()
                                .tabs
                                .ref_fg_browser()
                                .expect("no current browser")
                                .id;
                            match win_state.get_mut().tabs.find_browser(&id) {
                                Some(browser) => {
                                    browser.title = Some(format!("Exported {}", datatype).to_string());
                                }
                                None => warn!("Got message for unkown browser:  {:?}", id),
                            }
                        },
                    }
                },
                None => println!("Impossible to get your home dir!"),
            }
        }
    };
    Ok(())
}
