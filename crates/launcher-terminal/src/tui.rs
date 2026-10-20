use std::io;

use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use launcher_app::domain::{InstanceConfig, Loader};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, List, ListItem, ListState, Paragraph, Wrap},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Up,
    Down,
    Left,
    Right,
    Confirm,
    Back,
}

fn action(code: KeyCode) -> Option<Action> {
    match code {
        KeyCode::Char('w' | 'W') | KeyCode::Up => Some(Action::Up),
        KeyCode::Char('s' | 'S') | KeyCode::Down => Some(Action::Down),
        KeyCode::Char('a' | 'A') | KeyCode::Left => Some(Action::Left),
        KeyCode::Char('d' | 'D') | KeyCode::Right => Some(Action::Right),
        KeyCode::Enter | KeyCode::Char(' ') => Some(Action::Confirm),
        KeyCode::Esc => Some(Action::Back),
        _ => None,
    }
}

/// RAII restores on errors; Ratatui's installed panic hook restores before panic output.
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

pub async fn run(instances: Vec<InstanceConfig>) -> io::Result<()> {
    // The guard also covers a partial failure during terminal initialization.
    let _guard = TerminalGuard;
    let mut terminal = ratatui::try_init()?;
    let mut events = EventStream::new();
    let mut selected = 0;
    let mut detail = false;
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);
    loop {
        terminal.draw(|frame| render(frame, &instances, selected, detail))?;
        tokio::select! {
            result = &mut shutdown => { result?; break; }
            event = events.next() => match event {
                Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                        break;
                    }
                    match action(key.code) {
                        Some(Action::Up) if !instances.is_empty() => selected = selected.saturating_sub(1),
                        Some(Action::Down) if !instances.is_empty() => selected = (selected + 1).min(instances.len() - 1),
                        Some(Action::Left) => detail = false,
                        Some(Action::Right | Action::Confirm) => detail = !instances.is_empty(),
                        Some(Action::Back) if detail => detail = false,
                        Some(Action::Back) => break,
                        _ => {}
                    }
                }
                Some(Ok(_)) => {} // Resize also triggers a redraw on the next iteration.
                Some(Err(error)) => return Err(error),
                None => break,
            }
        }
    }
    Ok(())
}

fn render(frame: &mut Frame, instances: &[InstanceConfig], selected: usize, detail: bool) {
    let area = frame.area();
    if area.width < 24 || area.height < 7 {
        frame.render_widget(
            Paragraph::new("窗口过小\n请扩大终端\nEsc 返回").wrap(Wrap { trim: true }),
            area,
        );
        return;
    }
    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(3),
    ])
    .split(area);
    frame.render_widget(
        Paragraph::new("RTMCL · 框架阶段 / P0 待验证").block(Block::bordered()),
        rows[0],
    );
    let content = if area.width >= 72 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(rows[1])
    } else {
        Layout::vertical([Constraint::Percentage(45), Constraint::Percentage(55)]).split(rows[1])
    };
    let items: Vec<ListItem<'_>> = if instances.is_empty() {
        vec![ListItem::new("暂无实例；请使用 CLI 创建配置")]
    } else {
        instances
            .iter()
            .map(|instance| ListItem::new(instance.name.as_str()))
            .collect()
    };
    let list = List::new(items)
        .block(Block::bordered().title("实例"))
        .highlight_style(Style::default().fg(Color::Cyan))
        .highlight_symbol("> ");
    let mut state = ListState::default().with_selected((!instances.is_empty()).then_some(selected));
    frame.render_stateful_widget(list, content[0], &mut state);
    let text = if let Some(instance) = instances.get(selected) {
        let loader = match &instance.loader {
            Loader::Vanilla => "Vanilla".to_owned(),
            Loader::Fabric { version } => format!("Fabric {version}"),
        };
        format!(
            "{}\nID: {}\n游戏: {}\n加载器: {}\n安装: {:?}\n\n请用 P0 CLI 安装和启动；完整 TUI 待 P2。",
            instance.name, instance.id, instance.game_version, loader, instance.install_state
        )
    } else {
        "使用 instance create <名称> --game-version <版本> 创建实例配置。\n\n账户、安装和启动请使用 P0 CLI。".into()
    };
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: true })
            .block(Block::bordered().title(if detail {
                "详情 · 已聚焦"
            } else {
                "详情"
            })),
        content[1],
    );
    frame.render_widget(
        Paragraph::new("WASD / 方向键 导航 · Enter / 空格 确认 · Esc 返回")
            .wrap(Wrap { trim: true })
            .block(Block::bordered().title("任务：暂无运行任务")),
        rows[2],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn equivalent_navigation_keys() {
        for (letter, arrow) in [
            ('w', KeyCode::Up),
            ('s', KeyCode::Down),
            ('a', KeyCode::Left),
            ('d', KeyCode::Right),
        ] {
            assert!(action(KeyCode::Char(letter)) == action(arrow));
        }
        assert!(action(KeyCode::Enter) == action(KeyCode::Char(' ')));
    }

    #[test]
    fn renders_empty_and_narrow_terminals() {
        for (width, height) in [(100, 25), (40, 16), (10, 4), (1, 1)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| render(frame, &[], 0, false)).unwrap();
            let contents: String = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect();
            if width >= 24 && height >= 7 {
                assert!(contents.contains("P0"));
            } else {
                assert!(!contents.is_empty());
            }
        }
    }
}
