//! ratatui 界面渲染：三区布局 + 顶部状态栏 + 底部帮助栏。

use crate::app::{App, Focus, ReadState, SettingsItem, TravelDocField};
use crate::parse::ParsedCard;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};
use tarot_backend::reader::CardStatus;

/// 根据焦点返回边框样式。
fn border_style(app: &App, focus: Focus) -> Style {
    if app.focus == focus {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

/// 顶层绘制入口。
pub fn draw(f: &mut Frame, app: &App) {
    // 垂直分：状态栏(3) + 主体(min) + 帮助栏(1)
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(1),
        ])
        .split(f.area());

    draw_status_bar(f, app, outer[0]);
    draw_help_bar(f, app, outer[2]);

    // 主体水平分：左列(读卡器 + 解析/历史) | 右列(已保存卡片)
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(outer[1]);

    // 左列再纵向分：读卡器状态 | 解析数据
    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(7), Constraint::Min(5)])
        .split(body[0]);

    draw_readers(f, app, left[0]);
    draw_parsed(f, app, left[1]);
    draw_saved_cards(f, app, body[1]);

    // 旅行证件输入态：在中央叠加输入弹窗。
    if app.is_traveldoc_input() {
        draw_traveldoc_popup(f, app, f.area());
    }
    if app.is_settings_open() {
        draw_settings_popup(f, app, f.area());
    }
}

/// 居中弹窗区域计算。
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect {
        x,
        y,
        width: width.min(area.width),
        height: height.min(area.height),
    }
}

/// 旅行证件 MRZ 输入弹窗。
fn draw_traveldoc_popup(f: &mut Frame, app: &App, area: Rect) {
    let popup = centered_rect(60, 11, area);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(" 旅行证件 · 输入 MRZ 三要素 ")
        .borders(Borders::ALL)
        .border_style(
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        );

    let input = app.traveldoc_input.as_ref();
    let doc = input.map(|i| i.doc_number.as_str()).unwrap_or("");
    let dob = input.map(|i| i.dob.as_str()).unwrap_or("");
    let doe = input.map(|i| i.doe.as_str()).unwrap_or("");

    let field_line = |label: &str, val: &str, active: bool| -> Line<'static> {
        let cursor = if active { "_" } else { "" };
        let val_style = if active {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        let marker = if active { "▶ " } else { "  " };
        Line::from(vec![
            Span::styled(
                format!("{marker}{label}: "),
                Style::default().fg(if active { Color::Cyan } else { Color::DarkGray }),
            ),
            Span::styled(format!("{val}{cursor}"), val_style),
        ])
    };

    let lines = vec![
        Line::from(""),
        field_line(
            "证件号",
            doc,
            app.traveldoc_field == TravelDocField::DocNumber,
        ),
        field_line(
            "出生日期(YYMMDD)",
            dob,
            app.traveldoc_field == TravelDocField::Dob,
        ),
        field_line(
            "有效期(YYMMDD)",
            doe,
            app.traveldoc_field == TravelDocField::Doe,
        ),
        Line::from(""),
        Line::from(Span::styled(
            "  Tab/↑↓ 切换字段   Enter 读取   Esc 取消",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let p = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(p, popup);
}

/// 设置菜单弹窗。
fn draw_settings_popup(f: &mut Frame, app: &App, area: Rect) {
    let popup = centered_rect(56, 9, area);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(" 设置 ")
        .borders(Borders::ALL)
        .border_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );

    let setting_line = |item: SettingsItem, label: &str, enabled: bool| -> Line<'static> {
        let selected = app.selected_setting == item;
        let marker = if selected { "▶ " } else { "  " };
        let state = if enabled { "开" } else { "关" };
        let style = if selected {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        Line::from(vec![
            Span::styled(marker, style),
            Span::styled(format!("{label}: "), style),
            Span::styled(
                state,
                Style::default().fg(if enabled {
                    Color::Green
                } else {
                    Color::DarkGray
                }),
            ),
        ])
    };

    let lines = vec![
        Line::from(""),
        setting_line(SettingsItem::AutoPoll, "自动读卡", app.auto_poll),
        setting_line(SettingsItem::AutoSave, "自动保存", app.auto_save),
        Line::from(""),
        Line::from(Span::styled(
            "  ↑↓ 选择   Enter/Space 切换   Esc 关闭",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let p = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(p, popup);
}

/// 顶部状态栏：card_type / 卡片状态 / 消息。
fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let card_type = app
        .parsed
        .as_ref()
        .map(|p| p.card_type.as_str())
        .unwrap_or("—");
    let status_txt = match app.state {
        ReadState::Idle => "空闲",
        ReadState::CardPresent => "卡片在位",
        ReadState::Done => "读取完成",
        ReadState::Error(_) => "错误",
    };
    let auto = if app.auto_poll {
        "读卡:开"
    } else {
        "读卡:关"
    };
    let auto_save = if app.auto_save {
        "保存:开"
    } else {
        "保存:关"
    };
    let line = Line::from(vec![
        Span::styled(
            " tarot TUI ",
            Style::default().fg(Color::Black).bg(Color::Cyan),
        ),
        Span::raw(format!("  卡类型: {card_type}  ")),
        Span::styled(
            format!("[{status_txt}]"),
            Style::default().fg(status_color(&app.state)),
        ),
        Span::raw(format!("  {auto}  {auto_save}  ")),
        Span::styled(
            format!("| {}", app.message),
            Style::default().fg(Color::Yellow),
        ),
    ]);
    let p = Paragraph::new(line).block(Block::default().borders(Borders::ALL));
    f.render_widget(p, area);
}

fn status_color(state: &ReadState) -> Color {
    match state {
        ReadState::Done => Color::Green,
        ReadState::Error(_) => Color::Red,
        ReadState::CardPresent => Color::Yellow,
        ReadState::Idle => Color::Gray,
    }
}

/// 底部帮助栏。
fn draw_help_bar(f: &mut Frame, _app: &App, area: Rect) {
    let help = " q 退出 | s 保存 | , 设置 | Enter 读卡/历史 | t 旅行证件 | Tab 切区 | ↑↓ 滚动 ";
    let p = Paragraph::new(help).style(Style::default().fg(Color::DarkGray));
    f.render_widget(p, area);
}

/// 读卡器状态区。
fn draw_readers(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" 读卡器 ")
        .borders(Borders::ALL)
        .border_style(border_style(app, Focus::Readers));

    if let Some(err) = &app.init_error {
        let p = Paragraph::new(format!("PC/SC 不可用: {err}"))
            .style(Style::default().fg(Color::Red))
            .block(block)
            .wrap(Wrap { trim: true });
        f.render_widget(p, area);
        return;
    }

    if app.readers.is_empty() {
        let p = Paragraph::new("未检测到读卡器（插入后自动刷新）")
            .style(Style::default().fg(Color::DarkGray))
            .block(block);
        f.render_widget(p, area);
        return;
    }

    let items: Vec<ListItem> = app
        .readers
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let selected = i == app.selected_reader;
            let marker = if selected { "▶ " } else { "  " };
            let card = if selected {
                match app.card_status {
                    CardStatus::Present => " [卡片在位]",
                    CardStatus::Empty => " [空]",
                }
            } else {
                ""
            };
            let style = if selected {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            ListItem::new(format!("{marker}{r}{card}")).style(style)
        })
        .collect();

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

/// 解析数据区：按子卡分区展示卡类型/卡号/余额/有效期/交易记录。
fn draw_parsed(f: &mut Frame, app: &App, area: Rect) {
    let title = if app.history_view.is_some() {
        " 历史记录 (↑↓ 滚动) "
    } else {
        " 解析数据 (↑↓ 滚动) "
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style(app, Focus::Parsed));

    let mut lines: Vec<Line> = Vec::new();

    if let Some(idx) = app.history_view {
        draw_history_lines(app, idx, &mut lines);
        let p = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((app.parsed_scroll, 0));
        f.render_widget(p, area);
        return;
    }

    match &app.parsed {
        None => {
            lines.push(Line::from(Span::styled(
                "尚无数据，放卡或按 Enter 读取。",
                Style::default().fg(Color::DarkGray),
            )));
        }
        Some(result) => {
            if result.cards.len() > 1 {
                lines.push(Line::from(Span::styled(
                    format!("叠加卡：{} 张子卡", result.cards.len()),
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));
            }
            for (i, card) in result.cards.iter().enumerate() {
                render_card(&mut lines, i, card);
                lines.push(Line::from(""));
            }
            // ATR 附在末尾。
            lines.push(Line::from(Span::styled(
                format!("ATR: {}", result.atr),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    let p = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.parsed_scroll, 0));
    f.render_widget(p, area);
}

/// 把一张子卡渲染为多行。
fn render_card<'a>(lines: &mut Vec<Line<'a>>, idx: usize, card: &'a ParsedCard) {
    lines.push(Line::from(vec![
        Span::styled(
            format!("● {}", card.name),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  (#{})", idx + 1),
            Style::default().fg(Color::DarkGray),
        ),
    ]));

    if let Some(num) = &card.number {
        lines.push(kv_line("卡号", num));
    }
    let mut balance = card.balance.map(|bal| (card.currency.as_str(), bal));
    let mut monthly_total = None;
    for (k, v) in &card.fields {
        if k == "广州优惠金额累计" {
            monthly_total = Some(v.as_str());
            continue;
        }
        lines.push(kv_line(k, v));
    }

    for protocol in &card.protocols {
        if protocol.name != "交通联合" {
            if let Some(num) = &protocol.number {
                lines.push(kv_line(&format!("{}卡号", protocol.name), num));
            }
        }
        if protocol.name == "交通联合" && balance.is_none() {
            if let Some(bal) = protocol.balance {
                balance = Some((protocol.currency.as_str(), bal));
            }
        }
        for (k, v) in &protocol.fields {
            if k == "广州优惠金额累计" {
                monthly_total = Some(v.as_str());
                continue;
            }
            lines.push(kv_line(k, v));
        }
        for note in &protocol.notes {
            lines.push(Line::from(Span::styled(
                format!("  ! {note}"),
                Style::default().fg(Color::Red),
            )));
        }
    }

    if let Some((currency, bal)) = balance {
        lines.push(Line::from(vec![
            Span::styled("  余额: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{currency}{bal:.2}"),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    if let Some(total) = monthly_total {
        lines.push(kv_line("广州优惠金额累计", total));
    }

    // 交易记录（交通联合已把对应行程信息合并进每条记录，一一对应）。
    if !card.transactions.is_empty() {
        lines.push(Line::from(Span::styled(
            "  记录:",
            Style::default().fg(Color::Yellow),
        )));
        for t in &card.transactions {
            // 日期时间可能为空（非 BCD 字段），空时不显示。
            let dt = match (t.date.is_empty(), t.time.is_empty()) {
                (true, true) => String::new(),
                (false, true) => t.date.clone(),
                (true, false) => t.time.clone(),
                (false, false) => format!("{} {}", t.date, t.time),
            };
            // 符号：负为红，正为绿，0 不加符号。
            let (sign, color) = if t.amount < 0.0 {
                ("", Color::Red)
            } else if t.amount > 0.0 {
                ("+", Color::Green)
            } else {
                ("", Color::Gray)
            };
            // 类型：优先显示行程的进出站类型 + 辅助（地铁/公交），否则用交易类型。
            let kind_span = if !t.trip_kind.is_empty() {
                format!("{} [{}] ", t.trip_kind, t.aux)
            } else {
                format!("{} ", t.kind)
            };
            let mut spans = vec![
                Span::raw(format!("    {kind_span}")),
                Span::styled(
                    format!("{sign}{:.2}{} ", t.amount, card.currency),
                    Style::default().fg(color),
                ),
            ];
            // 交易后余额（若行程记录提供）。
            if let Some(bal) = t.balance_after {
                spans.push(Span::styled(
                    format!("余{:.2}{} ", bal, card.currency),
                    Style::default().fg(Color::Green),
                ));
            }
            if !t.source.is_empty() {
                spans.push(Span::styled(
                    format!("[{}] ", t.source),
                    Style::default().fg(Color::DarkGray),
                ));
            }
            spans.push(Span::styled(dt, Style::default().fg(Color::DarkGray)));
            lines.push(Line::from(spans));
        }
    }

    for note in &card.notes {
        lines.push(Line::from(Span::styled(
            format!("  ⚠ {note}"),
            Style::default().fg(Color::Red),
        )));
    }
}

/// 生成 "  key: value" 行。
fn kv_line(k: &str, v: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {k}: "), Style::default().fg(Color::Gray)),
        Span::styled(v.to_string(), Style::default().fg(Color::White)),
    ])
}

fn draw_history_lines<'a>(app: &'a App, idx: usize, lines: &mut Vec<Line<'a>>) {
    let Some(card) = app.saved_cards.get(idx) else {
        lines.push(Line::from(Span::styled(
            "未找到已保存卡片。",
            Style::default().fg(Color::DarkGray),
        )));
        return;
    };
    lines.push(Line::from(Span::styled(
        card.display_name(),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    let total = card
        .records
        .iter()
        .flat_map(|record| record.lines.iter())
        .filter(|line| line.starts_with("  "))
        .count();
    lines.push(Line::from(format!("共 {total} 条记录")));
    lines.push(Line::from(""));

    for record in &card.records {
        lines.push(Line::from(vec![
            Span::styled(
                "记录 ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{}  @{}", record.title, record.timestamp),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
        for line in &record.lines {
            lines.push(Line::from(format!("  {line}")));
        }
        lines.push(Line::from(""));
    }
}

/// 已保存卡片区：选择卡片后 Enter 在左侧查看历史。
fn draw_saved_cards(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(format!(" 已保存卡片 ({} 张) ", app.saved_cards.len()))
        .borders(Borders::ALL)
        .border_style(border_style(app, Focus::Saved));

    let mut lines: Vec<Line> = Vec::new();
    if app.saved_cards.is_empty() {
        lines.push(Line::from(Span::styled(
            "暂无保存记录。刷交通卡后按 s 保存。",
            Style::default().fg(Color::DarkGray),
        )));
    }
    for (i, card) in app.saved_cards.iter().enumerate() {
        let selected = i == app.selected_saved;
        let marker = if selected { "▶ " } else { "  " };
        let style = if selected {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        lines.push(Line::from(vec![
            Span::styled(marker, style),
            Span::styled(card.display_name(), style),
        ]));
        lines.push(Line::from(Span::styled(
            format!("    {} 条记录", card.records.len()),
            Style::default().fg(Color::DarkGray),
        )));
    }

    let p = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.saved_scroll, 0));
    f.render_widget(p, area);
}
