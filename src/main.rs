mod app;
mod backend;
mod config;
mod storage;

fn main() -> cosmic::iced::Result {
    cosmic::applet::run::<app::AppModel>(())
}
