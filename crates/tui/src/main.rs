//! tarot TUI 入口：初始化终端、运行事件循环、恢复终端。
//!
//! 三区布局（读卡器状态 / 解析数据 / 已保存卡片），支持轮询自动检测放卡/取卡。
//! 后端只抓原始字节，所有语义解析在 `parse` 模块完成。

mod app;
mod history;
mod parse;
mod settings;
mod ui;

use app::App;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use std::{
    io::{self, Stdout},
    time::Duration,
};

fn main() -> io::Result<()> {
    let mut terminal = setup_terminal()?;
    let mut app = App::new();
    let res = run(&mut terminal, &mut app);
    restore_terminal(&mut terminal)?;
    if let Err(e) = res {
        eprintln!("运行错误: {e}");
    }
    Ok(())
}

/// 进入 raw mode + 备用屏。
fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

/// 恢复终端到正常状态。
fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()
}

/// 主事件循环：绘制 -> 处理输入/轮询。
fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        // 轮询输入，带超时以便周期性 tick（卡片状态检测）。
        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    handle_key(app, key.code);
                }
            }
        }

        // 卡片状态轮询与自动读取。
        app.tick();

        if app.should_quit {
            return Ok(());
        }
    }
}

/// 键位处理。旅行证件输入态优先拦截，其余走普通模式。
fn handle_key(app: &mut App, code: KeyCode) {
    if app.is_traveldoc_input() {
        handle_traveldoc_key(app, code);
        return;
    }
    if app.is_settings_open() {
        handle_settings_key(app, code);
        return;
    }
    match code {
        KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
        KeyCode::Char('t') => app.start_traveldoc_input(),
        KeyCode::Enter => app.on_enter(),
        KeyCode::Char('s') => app.save_current_transit(),
        KeyCode::Char(',') => app.open_settings(),
        KeyCode::Tab => app.cycle_focus(),
        KeyCode::Up | KeyCode::Char('k') => app.on_up(),
        KeyCode::Down | KeyCode::Char('j') => app.on_down(),
        KeyCode::Left => {
            // 左方向：切到读卡器焦点便于选择。
            app.focus = app::Focus::Readers;
        }
        KeyCode::Right => {
            app.focus = app::Focus::Saved;
        }
        KeyCode::PageUp => app.on_page(true),
        KeyCode::PageDown => app.on_page(false),
        _ => {}
    }
}

/// 设置菜单键位：上下选择，Enter/Space 切换，Esc 关闭。
fn handle_settings_key(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc | KeyCode::Char(',') => app.close_settings(),
        KeyCode::Up | KeyCode::Char('k') => app.settings_prev_item(),
        KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => app.settings_next_item(),
        KeyCode::Enter | KeyCode::Char(' ') => app.toggle_selected_setting(),
        KeyCode::Char('q') => app.should_quit = true,
        _ => {}
    }
}

/// 旅行证件输入态键位：字符录入、字段切换、Enter 读取、Esc 取消。
fn handle_traveldoc_key(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc => app.cancel_traveldoc_input(),
        KeyCode::Enter => app.read_traveldoc_now(),
        KeyCode::Tab | KeyCode::Down => app.traveldoc_next_field(),
        KeyCode::Up => app.traveldoc_prev_field(),
        KeyCode::Backspace => app.traveldoc_backspace(),
        KeyCode::Char(c) if c.is_ascii_alphanumeric() || c == '<' => {
            app.traveldoc_push_char(c);
        }
        _ => {}
    }
}
