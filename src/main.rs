mod app;
mod backend;
mod cellular;
mod config;
mod storage;

fn main() -> cosmic::iced::Result {
    cosmic::applet::run::<app::AppModel>(())
}
