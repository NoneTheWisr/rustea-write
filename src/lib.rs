//! # Rustea
//!
//! `rustea` is a small crate for easily creating cross-platform TUI applications.
//! It is based off of the original [go-tea](https://github.com/tj/go-tea) created by TJ Holowaychuk.

pub mod view_helper;
pub extern crate crossterm;
pub mod command;

use std::{
    any::Any,
    io::{stdout, Result, Write},
    sync::mpsc::{self, Sender},
    thread,
};

use crossterm::event::{read, Event};

/// Any boxed type that may or may not contain data.
/// They are fed to your applications `update` method to tell it how and what to update.
///
/// Typically, you will use the `downcast_ref` method on your messages to determine the type of the message,
/// and extract the data from them if present.
///
/// # Example
///
/// ```
/// // the type of your message
/// struct HttpResponse(String);
///
/// // the boxed message itself
/// let http_response_message = Box::new(HttpResponse("Hello World".to_string()));
///
/// // determining the type of your message, and extracting the response
/// if let Some(res) = http_response_message.downcast_ref::<HttpResponse>() {
///     // do something with the response
///     // for example, setting it in the model to be rendered
///     model.response = Some(res);
/// }
/// ```
pub type Message = Box<dyn Any + Send>;

/// A boxed function or closure that performs computations and optionally dispatches messages.
/// All commands are processed in their own threads, so blocking commands are totally fine.
/// Frequently, data needs to be passed to commands. Since commands take no arguments,
/// a common solution to this is to build constructor functions.
///
/// # Example
///
/// ```
/// // a constructor function
/// fn make_request_command(url: &str) -> Command {
///     // it's okay to block since commands are multi threaded
///     let text_response = reqwest::blocking::get(url).unwrap().text().unwrap();
///     
///     // the command itself
///     Box::new(move || Some(Box::new(HttpResponse(text_response))))
/// }
pub type Command = Box<dyn FnOnce() -> Option<Message> + Send + 'static>;

/// Event representing a terminal resize (x, y).
/// Boxed as a message so it can be sent to the application.
pub struct ResizeEvent(pub u16, pub u16);

/// The trait your model must implement in order to be `run`.
///
/// `init` is called once when the model is run for the first time, and optionally returns a `Command`.
/// There is a default implementation of `init` that returns `None`.
///
/// `update` is called every time your application recieves a `Message`.
/// You are allowed to mutate your model's state in this function.
/// It optionally returns a `Command`.
///
/// `view` is called after every `update` and is responsible for rendering the model.
/// You are _not_ allowed to mutate the state of your application in the view, only render it.
///
/// For examples, check the `examples` directory.
pub trait App {
    fn init(&self) -> Option<Command> {
        None
    }

    fn update(&mut self, msg: Message) -> Option<Command>;
    fn view(&self, stdout: &mut impl Write);
}

/// Runs your application.
///
/// This will begin listening for keyboard events, and dispatching them to your application.
/// These keyboard events are handled by `crossterm`, and are fed into your `update` function as `Message`s.
/// You can access these keyboard events by simply downcasting them into a `crossterm::event::KeyEvent`.
///
/// `rustea` exports `crossterm`, so you can simply access it with `use rustea::crossterm`.
pub fn run(app: impl App) -> Result<()> {
    let mut app = app;
    let mut stdout = stdout();

    let (msg_tx, msg_rx) = mpsc::channel::<Message>();
    let msg_tx2 = msg_tx.clone();

    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>();
    let cmd_tx2 = cmd_tx.clone();

    thread::spawn(move || loop {
        match read().unwrap() {
            Event::Key(event) => msg_tx.send(Box::new(event)).unwrap(),
            Event::Mouse(event) => msg_tx.send(Box::new(event)).unwrap(),
            Event::Resize(x, y) => msg_tx.send(Box::new(ResizeEvent(x, y))).unwrap(),
        }
    });

    thread::spawn(move || loop {
        let cmd = match cmd_rx.recv() {
            Ok(cmd) => cmd,
            Err(_) => return,
        };

        let msg_tx2 = msg_tx2.clone();
        thread::spawn(move || {
            if let Some(msg) = cmd() {
                msg_tx2.send(msg).unwrap();
            }
        });
    });

    initialize(&app, cmd_tx2);
    app.view(&mut stdout);

    loop {
        let msg = msg_rx.recv().unwrap();
        if msg.is::<command::QuitMessage>() {
            break;
        } else if msg.is::<command::BatchMessage>() {
            let batch = msg.downcast::<command::BatchMessage>().unwrap();
            for cmd in batch.0 {
                cmd_tx.send(cmd).unwrap();
            }
        } else if let Some(cmd) = app.update(msg) {
            cmd_tx.send(cmd).unwrap();
        }

        app.view(&mut stdout);
    }

    Ok(())
}

fn initialize(app: &impl App, cmd_tx: Sender<Command>) {
    if let Some(cmd) = app.init() {
        cmd_tx.send(cmd).unwrap();
    }
}
