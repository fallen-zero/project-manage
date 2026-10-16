//! 日报管理：跨项目的工时条目（M10，spec §五）。四操作与判据在这里；
//! Excel 双向（Task 6/7）的读写两侧同在本文件：表头只认 `EXPORT_HEADERS`，日期与工时的界只认下面那几支纯函数。
//!
//! 日期形状判据**不重写一份**：表单侧走 `crate::todo::req_date`，导入的文本单元走 `crate::todo::is_iso_date`，
//! 两者读的同一条规则（先例：`milestone.rs:10` 用 `todo::norm_date`）。
//! `hours` 的界与精度也先于 SQL 拒掉：CHECK 违例冒出来是 db_failed，会把校验失败伪装成库故障。

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

use crate::error::{AppError, AppResult};
use crate::todo::{is_iso_date, req_date};

fn invalid(msg: &str) -> AppError {
    AppError::new("invalid_input", msg, Some("检查表单必填项"))
}

fn not_found(id: &str) -> AppError {
    AppError::new("not_found", &format!("记录不存在：{id}"), Some("刷新后重试"))
}

/// 单条工时上限。不是「一天不能超 24 小时」的会计规则（多条之和本来可以超），
/// 而是挡住手滑多打一个 0 的那一格。前端 `types/daily-report.ts` 有同名常量，两处各自钉值。
pub(crate) const MAX_HOURS_PER_ROW: f64 = 24.0;

/// 工时的界与精度（先于 SQL，理由同 `milestone::check_stage`）。
/// 两位小数用「放大 100 倍后离整数有多远」判，容差 `1e-9`：`0.1 * 100.0` 落在 `10.000000000000002`，
/// 差 `1.8e-15` 收；`7.535 * 100.0` 离最近整数差 `0.5` 拒。**别改成 `h.fract()`**，那条对 0.1 也判不出。
pub(crate) fn check_hours(h: f64) -> AppResult<f64> {
    let scaled = h * 100.0;
    if !h.is_finite() || h <= 0.0 || h > MAX_HOURS_PER_ROW || (scaled - scaled.round()).abs() > 1e-9 {
        return Err(AppError::new(
            "invalid_input",
            &format!("「工时」要大于 0、不超过 {MAX_HOURS_PER_ROW}、最多两位小数，收到 {h}"),
            Some("工时按小时数填，如 7.5"),
        ));
    }
    Ok(h)
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyReport {
    pub id: String,
    pub project_id: String,
    pub report_date: String,
    pub content: String,
    pub hours: f64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyReportInput {
    pub project_id: String,
    /// 收 `Option<String>` 是为了让「缺键」与「空白」走同一句点名错误（`req_date`）。
    pub report_date: Option<String>,
    pub content: String,
    pub hours: f64,
}

/// update 的入参**含** `project_id`：改归属在本轮做（spec §五.4，与 M9 的 `TodoEdit` 不同）。
/// 理由是项目归属是日报最容易填错的字段，且导入按编号/名称匹配、项目改名后要把导错的行挪走。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyReportEdit {
    pub project_id: String,
    pub report_date: Option<String>,
    pub content: String,
    pub hours: f64,
}

const SELECT_COLUMNS: &str =
    "d.id, d.project_id, d.report_date, d.content, d.hours, d.created_at, d.updated_at";

fn map_report(row: &rusqlite::Row<'_>) -> rusqlite::Result<DailyReport> {
    Ok(DailyReport {
        id: row.get(0)?,
        project_id: row.get(1)?,
        report_date: row.get(2)?,
        content: row.get(3)?,
        hours: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

/// 每个模块各留一份是本仓现形制（`todo.rs:101`、`milestone.rs:75` 已是两份），这里是第三份。
/// 不趁本轮顺手合并：合并要动那两格的既有测试，跟本轮目标无关，记进 Task 9 的欠账。
fn project_exists(conn: &Connection, project_id: &str) -> AppResult<()> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM projects WHERE id = ?1 AND deleted_at IS NULL",
        [project_id],
        |r| r.get(0),
    )?;
    if n == 0 {
        return Err(not_found(project_id));
    }
    Ok(())
}

/// **SQL 里出现任何按项目状态的过滤都算缺陷**（spec §五.3 / D6）：归档项目的日报照常列、照常算。
/// 与 M9 判据⑤（预警跳过归档）形状相反，是有意为之。变异 M① 打的就是这一格。
/// `?1 IS NULL OR d.project_id = ?1` 一形照抄 `todo::list`：**只一条 SQL**，不拆两条拼接。
pub fn list(conn: &Connection, project_id: Option<&str>) -> AppResult<Vec<DailyReport>> {
    let mut stmt = conn.prepare(
        &format!(
            "SELECT {SELECT_COLUMNS}
               FROM daily_reports d
               JOIN projects p ON p.id = d.project_id AND p.deleted_at IS NULL
              WHERE d.deleted_at IS NULL AND (?1 IS NULL OR d.project_id = ?1)
              ORDER BY d.report_date, d.created_at, d.id"
        ),
    )?;
    let rows = stmt.query_map(params![project_id], map_report)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<DailyReport> {
    let sql = format!(
        "SELECT {SELECT_COLUMNS} FROM daily_reports d WHERE d.id = ?1 AND d.deleted_at IS NULL"
    );
    conn.query_row(&sql, [id], map_report)
        .optional()?
        .ok_or_else(|| not_found(id))
}

pub fn create(conn: &Connection, input: &DailyReportInput) -> AppResult<DailyReport> {
    project_exists(conn, &input.project_id)?;
    let report_date = req_date("日报日期", input.report_date.clone())?;
    let content = input.content.trim().to_owned();
    if content.is_empty() {
        return Err(invalid("日报内容不能为空"));
    }
    let hours = check_hours(input.hours)?;
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO daily_reports (id, project_id, report_date, content, hours) VALUES (?1,?2,?3,?4,?5)",
        params![id, input.project_id, report_date, content, hours],
    )?;
    get(conn, &id)
}

pub fn update(conn: &Connection, id: &str, edit: &DailyReportEdit) -> AppResult<()> {
    project_exists(conn, &edit.project_id)?;
    let report_date = req_date("日报日期", edit.report_date.clone())?;
    let content = edit.content.trim().to_owned();
    if content.is_empty() {
        return Err(invalid("日报内容不能为空"));
    }
    let hours = check_hours(edit.hours)?;
    let changed = conn.execute(
        "UPDATE daily_reports SET project_id=?2, report_date=?3, content=?4, hours=?5, updated_at=datetime('now')
          WHERE id=?1 AND deleted_at IS NULL",
        params![id, edit.project_id, report_date, content, hours],
    )?;
    if changed == 0 {
        return Err(not_found(id));
    }
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    let changed = conn.execute(
        "UPDATE daily_reports SET deleted_at = datetime('now'), updated_at = datetime('now')
          WHERE id = ?1 AND deleted_at IS NULL",
        [id],
    )?;
    if changed == 0 {
        return Err(not_found(id));
    }
    Ok(())
}

/// 表头五列 —— 导出写它、导入按它定位列，**全仓只有这一处**（spec §六）。改一个词会同时打掉两格的
/// 测试：Task 6 的表头断言与 Task 7 的列定位断言读的都是这个常量。
pub const EXPORT_HEADERS: [&str; 5] = ["日期", "项目编号", "所属项目", "工作内容", "时长（小时）"];

/// 前端把**当前筛选后的行**传回来（spec §六.1：命令不收查询条件，免得在 SQL 里再造第二份日期比较）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportRow {
    pub report_date: String,
    /// `projects.code` 是可空列，原样给出；导入侧按它优先匹配，为空则退到名称。
    pub project_code: Option<String>,
    /// **必须是 `projects.name` 原值**，不是界面上的 `projectLabel`（后者拼了编号就对不上）。
    pub project_name: String,
    pub content: String,
    pub hours: f64,
}

/// 写 xlsx 的失败按文件域报：码与文案对齐 `error.rs:21` 的 `AppError::io`。
/// 不直接调 `io()` 是因为 `rust_xlsxwriter` 的错误不是 `std::io::Error`，硬套会丢掉它自己那句话。
fn write_fail(path: &Path, e: impl std::fmt::Display) -> AppError {
    AppError::new(
        "fs_failed",
        &format!("写 {} 失败：{e}", path.display()),
        Some("确认该路径存在且可读写"),
    )
}

/// 写一份真 `.xlsx`。0 行数据也写出表头 —— 那就是「下载模板」。返回落地的数据行数。
pub fn write_xlsx(path: &Path, rows: &[ExportRow]) -> AppResult<usize> {
    use rust_xlsxwriter::{Format, Workbook};

    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name("日报").map_err(|e| write_fail(path, e))?;
    ws.set_freeze_panes(1, 0).map_err(|e| write_fail(path, e))?;
    let bold = Format::new().set_bold();
    for (col, head) in EXPORT_HEADERS.iter().enumerate() {
        ws.write_with_format(0, col as u16, *head, &bold)
            .map_err(|e| write_fail(path, e))?;
    }
    for (i, r) in rows.iter().enumerate() {
        let row = (i + 1) as u32;
        ws.write(row, 0, r.report_date.as_str()).map_err(|e| write_fail(path, e))?;
        ws.write(row, 1, r.project_code.as_deref().unwrap_or(""))
            .map_err(|e| write_fail(path, e))?;
        ws.write(row, 2, r.project_name.as_str()).map_err(|e| write_fail(path, e))?;
        ws.write(row, 3, r.content.as_str()).map_err(|e| write_fail(path, e))?;
        // 工时写成数字（用户在 Excel 里要能求和），日期写成文本（上面那行）—— 两种类型是故意的，
        // 别顺手把日期也写成日期单元。
        ws.write(row, 4, r.hours).map_err(|e| write_fail(path, e))?;
    }
    wb.save(path).map_err(|e| write_fail(path, e))?;
    Ok(rows.len())
}

/// 一次导入最多多少行。上限是给「手滑选了整份年度台账」那一格兜底的，
/// 不是因为几百行会慢 —— 本机的物理内存会把这些行的校验与事务全压在一个 Mutex 上。
pub const MAX_IMPORT_ROWS: usize = 2000;

/// 一格读出来的形状。**保留 calamine 的类型区分**（文本 / 数字 / 日期单元 / 其它），
/// 因为 §六.2 的判据就是按类型分岔的：工时列出现日期单元要单独教用户怎么改。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Cell {
    Empty,
    Text(String),
    Number(f64),
    /// 年、月、日、时、分、秒。`to_ymd_hms_milli()` 是**单数 milli**（`calamine-0.36.1/src/datatype.rs:776`）；
    /// 不许改用它旁边的 `as_datetime()` —— 那返回 `chrono::NaiveDateTime`，一用就把 chrono 拖进代码面。
    DateTime(u16, u8, u8, u8, u8, u8),
    /// 布尔或错误单元：五列里任何一种都不认。
    Other,
}

/// `Data`（owned，`datatype.rs:36` 起）一共九臂 ⇒ 这个 match 必须写满九支。
/// 少一支编译器会报 non-exhaustive，多写不存在的臂会报 unreachable —— 臂数由编译器守着，别凭记忆删。
fn to_cell(d: &calamine::Data) -> Cell {
    use calamine::Data;
    match d {
        Data::Empty => Cell::Empty,
        Data::String(s) => Cell::Text(s.clone()),
        Data::Float(f) => Cell::Number(*f),
        Data::Int(i) => Cell::Number(*i as f64),
        Data::Bool(_) | Data::Error(_) => Cell::Other,
        Data::DateTime(e) => {
            let (y, mo, d, hh, mm, ss, _milli) = e.to_ymd_hms_milli();
            Cell::DateTime(y, mo, d, hh, mm, ss)
        }
        // xlsb/ods 才给 ISO 形状；退化成文本，交给 `date_of_cell` 的形状判据。
        Data::DateTimeIso(s) | Data::DurationIso(s) => Cell::Text(s.clone()),
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SheetRow {
    /// **Excel 里的绝对行号**（表头是第 1 行）。用户看的是 Excel，报 0-based 下标等于没报。
    pub line: u32,
    pub date: Cell,
    pub code: Cell,
    pub name: Cell,
    pub content: Cell,
    pub hours: Cell,
}

/// 一行里某一个字段的拒绝。`field` 用表头那五个词，与界面上的列名同词。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reject {
    pub line: u32,
    pub field: String,
    pub reason: String,
}

fn reject(line: u32, field: &str, reason: &str) -> Reject {
    Reject { line, field: field.to_owned(), reason: reason.to_owned() }
}

/// 通过校验、可以往库里写的一行。**不带 `line`**：行号只在拒绝时用（`Reject.line`），
/// 写库这一步不需要它 —— 带上就是一条没人读的字段，会逼出第二条 `dead_code` 豁免。
#[derive(Debug, Clone)]
pub(crate) struct ReadyRow {
    pub project_id: String,
    pub report_date: String,
    pub content: String,
    pub hours: f64,
}

pub(crate) struct ProjectRow {
    pub id: String,
    pub code: Option<String>,
    pub name: String,
}

/// 单元格 → 用来匹配的文本。整数形状的数字不留 `.0` 尾巴（Excel 常把编号列识别成数字）。
/// **已知边界**：`"01"` 这种前导零的编号在 Excel 里会变成 `1`，本函数不补救 —— 那种项目要用编号列
/// 匹配不上时，退到名称那列仍然能命中（`project_of_cells` 的两级次序就是为这个存在的）。
fn text_of(cell: &Cell) -> Option<String> {
    match cell {
        Cell::Empty => None,
        Cell::Text(s) => {
            let t = s.trim();
            (!t.is_empty()).then(|| t.to_owned())
        }
        Cell::Number(f) => {
            if f.fract() == 0.0 {
                Some(format!("{}", *f as i64))
            } else {
                Some(f.to_string())
            }
        }
        _ => None,
    }
}

/// `(日期串, 是否截掉了时分秒)`。判据收在纯函数里（M8 终审 C1 教训③），所以 `Cell::DateTime`
/// 这一支不必造出真日期格式单元也能测到。形状只走 `is_iso_date`，**历法有效性不在本轮**。
fn date_of_cell(cell: &Cell, line: u32) -> Result<(String, bool), Reject> {
    match cell {
        Cell::Text(s) => {
            let t = s.trim();
            if is_iso_date(t) {
                Ok((t.to_owned(), false))
            } else {
                Err(reject(line, "日期", &format!("不是 YYYY-MM-DD：「{t}」")))
            }
        }
        Cell::DateTime(y, mo, d, hh, mm, ss) => Ok((
            format!("{y:04}-{mo:02}-{d:02}"),
            *hh != 0 || *mm != 0 || *ss != 0,
        )),
        Cell::Number(_) => Err(reject(
            line,
            "日期",
            "日期列不要填数字；要填 2026-10-16 这样的文本或日期格式",
        )),
        Cell::Empty => Err(reject(line, "日期", "日期不能空着")),
        Cell::Other => Err(reject(line, "日期", "这一格不是日期")),
    }
}

/// 工时：数字单元直接进 `check_hours`；文本只认「全是数字与小数点」的形状，
/// 所以全角（`８`）、千分位（`1,5`）、科学计数（`1e2`）都在这里被挡在 parse 之前。
/// 日期单元 ⇒ 单独一句教用户改（`8:30` 是时间格式，本表要的是小时数）。
fn hours_of_cell(cell: &Cell, line: u32) -> Result<f64, Reject> {
    match cell {
        Cell::Number(f) => check_hours(*f)
            .map_err(|e| reject(line, "工时", &format!("{}（要大于 0、不超过 {MAX_HOURS_PER_ROW}、最多两位小数）", e.message))),
        Cell::Text(s) => {
            let t = s.trim();
            if t.chars().any(|c| !(c.is_ascii_digit() || c == '.')) {
                return Err(reject(line, "工时", &format!("工时要填小时数（如 8.5），收到「{t}」")));
            }
            match t.parse::<f64>() {
                Ok(f) => check_hours(f)
                    .map_err(|e| reject(line, "工时", &format!("{}（要大于 0、不超过 {MAX_HOURS_PER_ROW}、最多两位小数）", e.message))),
                Err(_) => Err(reject(line, "工时", &format!("工时不是数字：「{t}」"))),
            }
        }
        Cell::DateTime(..) => Err(reject(
            line,
            "工时",
            "工时要填小时数（8.5），不要用时间格式（8:30）",
        )),
        Cell::Empty => Err(reject(line, "工时", "工时不能空着")),
        Cell::Other => Err(reject(line, "工时", "这一格不是数字")),
    }
}

fn project_of_cells(code: &Cell, name: &Cell, line: u32, projects: &[ProjectRow]) -> Result<String, Reject> {
    // 两级：**先编号、命中即用**；编号为空或编号匹配不到 ⇒ 退到名称。
    // 退到名称而不是直接报错，是为了「用户改了编号列 / 项目没填编号」两种常态还能走通。
    if let Some(c) = text_of(code) {
        let hits: Vec<&ProjectRow> = projects.iter().filter(|p| p.code.as_deref() == Some(c.as_str())).collect();
        match hits.len() {
            1 => return Ok(hits[0].id.clone()),
            n if n > 1 => return Err(reject(line, "项目编号", &format!("编号「{c}」对应 {n} 个项目，请改用所属项目列"))),
            _ => {}
        }
    }
    match text_of(name) {
        None => Err(reject(line, "所属项目", "项目编号与所属项目不能同时空着")),
        Some(n) => {
            let hits: Vec<&ProjectRow> = projects.iter().filter(|p| p.name == n).collect();
            match hits.len() {
                0 => Err(reject(line, "所属项目", &format!("找不到项目「{n}」（要先在项目页建好，或改填项目编号）"))),
                1 => Ok(hits[0].id.clone()),
                k => Err(reject(line, "所属项目", &format!("项目名「{n}」重名 {k} 个，请改填项目编号"))),
            }
        }
    }
}

/// 一行 → 可写入的 `ReadyRow`，或**第一个**拒绝理由。
/// `preview` 与 `apply` 共用这一支：两段式里两处判据必须同源（spec §六.2）。
fn validate_row(r: &SheetRow, projects: &[ProjectRow]) -> Result<ReadyRow, Reject> {
    let (report_date, _truncated) = date_of_cell(&r.date, r.line)?;
    let content = text_of(&r.content).ok_or_else(|| reject(r.line, "工作内容", "工作内容不能空着"))?;
    let hours = hours_of_cell(&r.hours, r.line)?;
    let project_id = project_of_cells(&r.code, &r.name, r.line, projects)?;
    Ok(ReadyRow { project_id, report_date, content, hours })
}

/// 五列在表里的下标（按表头文字定位，**不是列序**）。
#[derive(Debug, Clone, Copy)]
struct Columns {
    date: usize,
    code: usize,
    name: usize,
    content: usize,
    hours: usize,
}

/// 打不开表按 `extract_failed` 报，message 与 `extract.rs:179` 同码同文案（本轮不开新码、不改那句）。
/// 所以路径进不了 message，参数只留作调用点的记录（下划线前缀是读数逼出来的，不是笔误）。
fn read_fail(_path: &Path, e: impl std::fmt::Display) -> AppError {
    AppError::new(
        "extract_failed",
        &format!("打开表格失败：{e}"),
        Some("文件可能已损坏、受密码保护，或是伪装成表格的其它格式"),
    )
}

/// 读第一个工作表。多 sheet 只读第一个（spec §十一）。
pub(crate) fn read_rows(path: &Path) -> AppResult<Vec<SheetRow>> {
    use calamine::Reader as _; // `open_workbook_auto` 后必须带这条（先例 `extract.rs:176`）

    let mut book = calamine::open_workbook_auto(path).map_err(|e| read_fail(path, e))?;
    let names = book.sheet_names();
    let Some(first) = names.first() else {
        return Err(invalid("这份工作簿里没有工作表"));
    };
    let range = book.worksheet_range(first).map_err(|e| read_fail(path, e))?;
    let mut it = range.rows();
    let Some(header) = it.next() else {
        return Err(invalid("这份表是空的，至少要有表头一行（可以先「下载模板」）"));
    };
    let cols = locate_columns(header)?;

    // `Range::start()` 给的是绝对 0-based 左上角行号（`calamine-0.36.1/src/lib.rs:784`）。
    // Excel 显示的行号从 1 起，表头占第一行 ⇒ 数据的绝对行号 = start + 1 + (下标 + 1)。
    let base = range.start().map(|(r, _)| r).unwrap_or(0) + 1;
    let mut out = Vec::new();
    for (i, row) in it.enumerate() {
        let pick = |idx: usize| row.get(idx).map(to_cell).unwrap_or(Cell::Empty);
        let line = base + i as u32 + 1;
        // 整行全空 ⇒ 跳过：用户在 Excel 里删过内容但留了空行，是常态，不该变成一条拒绝。
        if matches!(
            (pick(cols.date), pick(cols.code), pick(cols.name), pick(cols.content), pick(cols.hours)),
            (Cell::Empty, Cell::Empty, Cell::Empty, Cell::Empty, Cell::Empty)
        ) {
            continue;
        }
        out.push(SheetRow {
            line,
            date: pick(cols.date),
            code: pick(cols.code),
            name: pick(cols.name),
            content: pick(cols.content),
            hours: pick(cols.hours),
        });
    }
    if out.len() > MAX_IMPORT_ROWS {
        return Err(AppError::new(
            "invalid_input",
            &format!("一次最多导入 {MAX_IMPORT_ROWS} 行，这份有 {} 行，请分批", out.len()),
            Some("用页面上的日期范围筛小一点再导出、再导入"),
        ));
    }
    Ok(out)
}

/// **按表头文字定位列**：插一列、换列序都不该让导入错位（M⑧ 的靶子就是「改成按列序硬数」）。
/// 认不出必需列 ⇒ 点名缺哪几列。
fn locate_columns(header: &[calamine::Data]) -> AppResult<Columns> {
    let texts: Vec<String> = header.iter().map(|c| c.to_string().trim().to_owned()).collect();
    let mut found: [Option<usize>; 5] = [None; 5];
    for (i, t) in texts.iter().enumerate() {
        for (slot, head) in EXPORT_HEADERS.iter().enumerate() {
            if t == *head && found[slot].is_none() {
                found[slot] = Some(i);
            }
        }
    }
    let missing: Vec<&str> = EXPORT_HEADERS
        .iter()
        .enumerate()
        .filter(|(slot, _)| found[*slot].is_none())
        .map(|(_, head)| *head)
        .collect();
    if !missing.is_empty() {
        return Err(invalid(&format!(
            "表头缺列：{}。要用「下载模板」出来的那五列 —— 列的顺序可以变、名字不能改",
            missing.join("、")
        )));
    }
    Ok(Columns {
        date: found[0].unwrap(),
        code: found[1].unwrap(),
        name: found[2].unwrap(),
        content: found[3].unwrap(),
        hours: found[4].unwrap(),
    })
}

/// 匹配用的项目清单：软删项目**不在**清单里（挂不上新行，与 `create` 同一判据）。
fn load_projects(conn: &Connection) -> AppResult<Vec<ProjectRow>> {
    let mut stmt = conn.prepare("SELECT id, code, name FROM projects WHERE deleted_at IS NULL")?;
    let rows = stmt.query_map([], |r| {
        Ok(ProjectRow { id: r.get(0)?, code: r.get(1)?, name: r.get(2)? })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 查重用的键集合。**f64 不进 hash**：换成放大 100 倍的整数（`check_hours` 已保证两位小数）。
fn existing_keys(conn: &Connection) -> AppResult<HashSet<(String, String, String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT report_date, project_id, content, hours FROM daily_reports WHERE deleted_at IS NULL",
    )?;
    let rows = stmt.query_map([], |r| {
        let h: f64 = r.get(3)?;
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            (h * 100.0).round() as i64,
        ))
    })?;
    let mut out = HashSet::new();
    for r in rows {
        out.insert(r?);
    }
    Ok(out)
}

/// 预检的结果。**只读、不写库**（D5）：界面上先给用户看清「几行能进、哪几行为什么不进」，
/// 再决定要不要点确认。重复行只提示、不拦（D2 之后「同一天同项目同内容同工时」是合法的）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub total: usize,
    pub ok: usize,
    pub duplicates: usize,
    pub duplicate_lines: Vec<u32>,
    /// 日期单元带时分秒、被截到「日」的那些行 —— 不静默（spec §六.2）。
    pub truncated_lines: Vec<u32>,
    pub rejected: Vec<Reject>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOutcome {
    pub inserted: usize,
}

pub fn preview(conn: &Connection, path: &Path) -> AppResult<ImportPreview> {
    let rows = read_rows(path)?;
    let projects = load_projects(conn)?;
    let existing = existing_keys(conn)?;
    let mut ok = 0usize;
    let mut rejected: Vec<Reject> = Vec::new();
    let mut duplicate_lines: Vec<u32> = Vec::new();
    let mut truncated_lines: Vec<u32> = Vec::new();
    for r in &rows {
        let (_date, truncated) = match date_of_cell(&r.date, r.line) {
            Ok(v) => v,
            Err(e) => {
                rejected.push(e);
                continue;
            }
        };
        if truncated {
            truncated_lines.push(r.line);
        }
        // 其余三段逐个拿第一个拒绝理由，与 `validate_row` 同次序（同一支判据，不重拼第二份）：
        let ready = match validate_row(r, &projects) {
            Ok(v) => v,
            Err(e) => {
                rejected.push(e);
                continue;
            }
        };
        ok += 1;
        if existing.contains(&(ready.report_date, ready.project_id, ready.content, (ready.hours * 100.0).round() as i64)) {
            duplicate_lines.push(r.line);
        }
    }
    Ok(ImportPreview {
        total: rows.len(),
        ok,
        duplicates: duplicate_lines.len(),
        duplicate_lines,
        truncated_lines,
        rejected,
    })
}

/// 落库：**重新解析同一个路径**（不缓存 preview 的结果，也不锁文件 —— D5 当次写明的代价），
/// 重新校验，任一行不合格 ⇒ 一句 `invalid_input` 报出「第几行 + 哪个字段 + 为什么」，**库里一条都不许多**。
pub fn apply(conn: &mut Connection, path: &Path) -> AppResult<ImportOutcome> {
    let rows = read_rows(path)?;
    let projects = load_projects(conn)?;
    let mut ready: Vec<ReadyRow> = Vec::with_capacity(rows.len());
    for r in &rows {
        match validate_row(r, &projects) {
            Ok(v) => ready.push(v),
            Err(e) => {
                return Err(AppError::new(
                    "invalid_input",
                    &format!("第 {} 行的{}不合格：{}", e.line, e.field, e.reason),
                    Some("修好整份文件后重新预检；任何一行不合格都不写入"),
                ))
            }
        }
    }
    if ready.is_empty() {
        return Err(invalid("没有可导入的行（这份表只有表头？）"));
    }
    let tx = conn.transaction()?;
    for r in &ready {
        tx.execute(
            "INSERT INTO daily_reports (id, project_id, report_date, content, hours) VALUES (?1,?2,?3,?4,?5)",
            params![uuid::Uuid::new_v4().to_string(), r.project_id, r.report_date, r.content, r.hours],
        )?;
    }
    // `Transaction` 的 drop 默认回滚（`rusqlite-0.40.2/src/transaction.rs` 里 `DropBehavior::Rollback`
    // 那句注释），所以这里只要不 `commit` 就是整批撤销；不需要手写 rollback 分支（先例 `db::apply_version`）。
    tx.commit()?;
    Ok(ImportOutcome { inserted: ready.len() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use rusqlite::params;

    fn seed(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO projects (id, name, status) VALUES (?1, '日报测试项目', '立项')",
            [id],
        )
        .unwrap();
    }

    /// D6 的靶子夹具：归档状态取自 `projects` 的 8 段枚举（`db.rs` V2），词表外的值会被 CHECK 挡。
    fn seed_archived(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO projects (id, name, status) VALUES (?1, '已归档项目', '归档')",
            [id],
        )
        .unwrap();
    }

    fn soft_delete_project(conn: &Connection, id: &str) {
        conn.execute("UPDATE projects SET deleted_at = datetime('now') WHERE id = ?1", [id])
            .unwrap();
    }

    fn input(project_id: &str, date: Option<&str>, content: &str, hours: f64) -> DailyReportInput {
        DailyReportInput {
            project_id: project_id.to_owned(),
            report_date: date.map(|s| s.to_owned()),
            content: content.to_owned(),
            hours,
        }
    }

    #[test]
    fn check_hours_accepts_the_bounds_and_two_decimals() {
        for h in [0.1_f64, 0.01, 7.5, 7.53, 24.0] {
            assert_eq!(check_hours(h).unwrap(), h, "{h} 要收");
        }
    }

    #[test]
    fn check_hours_refuses_zero_out_of_range_and_sub_cent_precision() {
        for h in [0.0_f64, -1.0, 24.01, 100.0, 0.001, 7.535] {
            let err = check_hours(h).unwrap_err();
            assert_eq!(err.code, "invalid_input", "{h} 要在进 SQL 之前拒掉");
            assert!(err.message.contains("工时"), "错误文本要点名「工时」：{}", err.message);
        }
    }

    #[test]
    fn create_requires_an_existing_undeleted_project() {
        let conn = db::open_in_memory().unwrap();
        let err = create(&conn, &input("nope", Some("2026-10-16"), "甲", 1.0)).unwrap_err();
        assert_eq!(err.code, "not_found", "项目不存在按 not_found(项目 id) 报，与 todo 同形");
        seed(&conn, "p-gone");
        soft_delete_project(&conn, "p-gone");
        assert_eq!(
            create(&conn, &input("p-gone", Some("2026-10-16"), "乙", 1.0)).unwrap_err().code,
            "not_found",
            "软删项目也不能挂新日报"
        );
    }

    #[test]
    fn create_rejects_empty_content() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let err = create(&conn, &input("p1", Some("2026-10-16"), "   ", 1.0)).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM daily_reports", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "被拒的输入不许留下行");
    }

    /// **M② 的靶子**：若哪天有人把 `req_date` 换成 `norm_date`，空白会折成 NULL 走进 SQL，
    /// 报错变成 db_failed（V6 那条 NOT NULL 测试就是它的另一面），这条当场红。
    #[test]
    fn create_rejects_blank_and_absent_report_date_naming_the_column() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        for date in [None, Some(""), Some("   ")] {
            let err = create(&conn, &input("p1", date, "甲", 1.0)).unwrap_err();
            assert_eq!(err.code, "invalid_input", "{date:?} 要报校验错，不是库错");
            assert!(err.message.contains("日报日期"), "{}", err.message);
        }
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM daily_reports", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn create_rejects_a_non_iso_report_date() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let err = create(&conn, &input("p1", Some("2026-6-30"), "甲", 1.0)).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert_eq!(err.hint.as_deref(), Some("日期要用 YYYY-MM-DD"));
        assert!(err.message.contains("日报日期"), "{}", err.message);
    }

    #[test]
    fn create_rejects_out_of_range_hours() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        assert_eq!(create(&conn, &input("p1", Some("2026-10-16"), "甲", 30.0)).unwrap_err().code, "invalid_input");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM daily_reports", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "工时被拒也不许留下行");
    }

    #[test]
    fn create_then_read_roundtrip() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let r = create(&conn, &input("p1", Some(" 2026-10-16 "), "  联调第三方接口  ", 7.5)).unwrap();
        assert_eq!(r.content, "联调第三方接口", "写入要 trim");
        assert_eq!(r.report_date, "2026-10-16", "日期两侧都要 trim 后进判据");
        assert_eq!(r.hours, 7.5);
        assert_eq!(get(&conn, &r.id).unwrap(), r, "create 回来的行必须与再读的一致");
        assert_eq!(list(&conn, Some("p1")).unwrap().len(), 1);
        assert_eq!(list(&conn, None).unwrap().len(), 1, "None = 跨项目全部");
    }

    /// 三级排序键一起钉：`report_date` 是主键（按天填的直觉），后两级是 M9 N3 那条同秒 tiebreaker。
    /// 直写 INSERT 而不走 create()：uuid 主键给不出可控 id 次序，同秒 created_at 也只能手写。
    #[test]
    fn list_orders_by_report_date_then_created_at_then_id() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        for (id, date, created) in [
            ("d-b", "2026-10-15", "2026-10-15 09:00:00"),
            ("d-a", "2026-10-15", "2026-10-15 09:00:00"),
            ("d-c", "2026-10-14", "2026-10-14 23:00:00"),
        ] {
            conn.execute(
                "INSERT INTO daily_reports (id, project_id, report_date, content, hours, created_at)
                 VALUES (?1, 'p1', ?2, ?1, 1.0, ?3)",
                params![id, date, created],
            )
            .unwrap();
        }
        let ids: Vec<String> = list(&conn, None).unwrap().into_iter().map(|r| r.id).collect();
        assert_eq!(ids.join(","), "d-c,d-a,d-b", "先按日期（10-14 在前），同日同秒按 id 升序；插入序是 b、a、c");
    }

    #[test]
    fn list_hides_soft_deleted_reports() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let r = create(&conn, &input("p1", Some("2026-10-16"), "甲", 1.0)).unwrap();
        delete(&conn, &r.id).unwrap();
        assert!(list(&conn, None).unwrap().is_empty());
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM daily_reports WHERE deleted_at IS NOT NULL", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "delete 是软删：行要在库里（延续 M9 D8 的形制）");
        assert_eq!(get(&conn, &r.id).unwrap_err().code, "not_found");
    }

    #[test]
    fn cross_project_list_hides_reports_of_a_soft_deleted_project() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p-alive");
        seed(&conn, "p-dead");
        create(&conn, &input("p-alive", Some("2026-10-16"), "活项目的日报", 1.0)).unwrap();
        let dead = create(&conn, &input("p-dead", Some("2026-10-16"), "已删项目的日报", 1.0)).unwrap();
        soft_delete_project(&conn, "p-dead");

        let contents: Vec<String> = list(&conn, None).unwrap().into_iter().map(|r| r.content).collect();
        assert_eq!(contents, vec!["活项目的日报"], "软删项目的日报不能出现在跨项目列表里");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM daily_reports WHERE id = ?1", [&dead.id], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "项目软删不许动日报的行（恢复项目即原样回来）");
        assert!(list(&conn, Some("p-dead")).unwrap().is_empty(), "指定已删项目也读不到");
    }

    /// **D6 的兑现点，也是变异 M① 的靶子**：给 `list` 顺手加一句 `AND p.status <> '归档'`，
    /// 这条当场红。归档项目的工时照常列、照常算 —— 跳过归档等于给领导的台账凭空少一截。
    #[test]
    fn list_includes_rows_of_an_archived_project() {
        let conn = db::open_in_memory().unwrap();
        seed_archived(&conn, "p-old");
        let r = create(&conn, &input("p-old", Some("2026-03-02"), "收尾交付物归档", 3.5)).unwrap();
        let all = list(&conn, None).unwrap();
        assert_eq!(all.len(), 1, "归档项目的日报要在跨项目列表里");
        assert_eq!(all[0].id, r.id);
        assert_eq!(list(&conn, Some("p-old")).unwrap().len(), 1, "按归档项目筛也要读到");
    }

    #[test]
    fn update_can_move_a_row_to_another_project() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        seed(&conn, "p2");
        let r = create(&conn, &input("p1", Some("2026-10-16"), "甲", 1.0)).unwrap();
        update(
            &conn,
            &r.id,
            &DailyReportEdit {
                project_id: "p2".to_owned(),
                report_date: Some("2026-10-17".to_owned()),
                content: "乙".to_owned(),
                hours: 2.0,
            },
        )
        .unwrap();
        let after = get(&conn, &r.id).unwrap();
        assert_eq!(after.project_id, "p2", "改归属在本轮做（spec §五.4，与 TodoEdit 不同）");
        assert_eq!(after.report_date, "2026-10-17");
        assert_eq!(after.content, "乙");
        assert_eq!(after.hours, 2.0);
    }

    #[test]
    fn update_refuses_a_missing_or_soft_deleted_project_and_bad_fields() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        seed(&conn, "p-gone");
        soft_delete_project(&conn, "p-gone");
        let r = create(&conn, &input("p1", Some("2026-10-16"), "甲", 1.0)).unwrap();
        let edit = |pid: &str, date: &str, hours: f64| DailyReportEdit {
            project_id: pid.to_owned(),
            report_date: Some(date.to_owned()),
            content: "乙".to_owned(),
            hours,
        };
        assert_eq!(update(&conn, &r.id, &edit("p-gone", "2026-10-16", 1.0)).unwrap_err().code, "not_found");
        assert_eq!(update(&conn, &r.id, &edit("p1", "2026-10-16", 99.0)).unwrap_err().code, "invalid_input");
        assert_eq!(update(&conn, &r.id, &edit("p1", "10-16", 1.0)).unwrap_err().code, "invalid_input");
        assert_eq!(update(&conn, "no-such-id", &edit("p1", "2026-10-16", 1.0)).unwrap_err().code, "not_found");
        assert_eq!(get(&conn, &r.id).unwrap().content, "甲", "被拒的 update 一条都不许改着");
    }

    #[test]
    fn delete_is_soft_and_a_second_delete_reports_not_found() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let r = create(&conn, &input("p1", Some("2026-10-16"), "甲", 1.0)).unwrap();
        delete(&conn, &r.id).unwrap();
        assert_eq!(
            delete(&conn, &r.id).unwrap_err().code,
            "not_found",
            "二次删要响亮，不能静默成功"
        );
    }

    /// 线格式钉键数（本仓形制：`todo.rs:424` 七键、manifest 八键、SearchBundle 七键）。
    #[test]
    fn daily_report_line_format_has_exactly_seven_keys() {
        let r = DailyReport {
            id: "d1".to_owned(),
            project_id: "p1".to_owned(),
            report_date: "2026-10-16".to_owned(),
            content: "甲".to_owned(),
            hours: 7.5,
            created_at: "2026-10-16 00:00:00".to_owned(),
            updated_at: "2026-10-16 00:00:00".to_owned(),
        };
        let obj = serde_json::to_value(&r).unwrap();
        let mut keys: Vec<&str> = obj.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec!["content", "createdAt", "hours", "id", "projectId", "reportDate", "updatedAt"],
            "DailyReport 的键集合变了就要同步 src/types/daily-report.ts 与 xlsx 表头"
        );
    }

    /// 读回用 calamine（与本任务的写同一把库），**不经 `extract::sheet_text`**：
    /// 那个函数把所有格子拼成一串文本，看不出「这一格是日期单元还是文本」，而那是 §六.1 的判据。
    ///
    /// **但 `to_string()` 也分不出类型**（控制方按包缓存核过）：`Data::Float(7.5)` 与
    /// `Data::String("7.5")` 都印成 `7.5`（`calamine-0.36.1/src/datatype.rs:212-216`），
    /// `Data::DateTime(e)` 走 `ExcelDateTime` 的 Display，而它印的就是裸 `self.value`
    /// （`datatype.rs:986-990`）—— 序列号 `46311`，**不带时分秒**。
    /// 所以凡要钉「这一格是数字还是文本」的判据，必须另按**变体**断（下面的 `read_typed`），
    /// 只比字符串会得到一条打不红的假守护（Task 6 复审就是这么打回第一版的）。
    fn read_cells(path: &std::path::Path) -> Vec<Vec<String>> {
        use calamine::Reader as _;
        let mut book = calamine::open_workbook_auto(path).unwrap();
        let names = book.sheet_names();
        let range = book.worksheet_range(&names[0]).unwrap();
        range.rows().map(|r| r.iter().map(|c| c.to_string()).collect()).collect()
    }

    /// 按**变体**读回：字符串读法证不了类型，这一支才证得了（复审 Important）。
    fn read_typed(path: &std::path::Path) -> Vec<Vec<calamine::Data>> {
        use calamine::Reader as _;
        let mut book = calamine::open_workbook_auto(path).unwrap();
        let names = book.sheet_names();
        let range = book.worksheet_range(&names[0]).unwrap();
        range.rows().map(|r| r.to_vec()).collect()
    }

    /// calamine 对数值单元可能给 `Float` 也可能给 `Int`（取决于它解析出什么），
    /// 所以「是数字」的判据走 `DataType::as_f64()`，而不是 `matches!(.., Data::Float(_))`。
    fn is_numeric(d: &calamine::Data) -> bool {
        use calamine::DataType as _;
        d.as_f64().is_some() && !matches!(d, calamine::Data::String(_))
    }

    /// 工作表名也要能断到（`read_cells`/`read_typed` 都是盲取 `names[0]`）。
    fn first_sheet_name(path: &std::path::Path) -> String {
        use calamine::Reader as _;
        let book = calamine::open_workbook_auto(path).unwrap();
        book.sheet_names().first().cloned().unwrap_or_default()
    }

    fn export_row(name: &str, code: Option<&str>) -> ExportRow {
        ExportRow {
            report_date: "2026-10-16".to_owned(),
            project_code: code.map(|s| s.to_owned()),
            project_name: name.to_owned(),
            content: "联调第三方接口".to_owned(),
            hours: 7.5,
        }
    }

    #[test]
    fn export_writes_a_real_xlsx_that_calamine_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("out.xlsx");
        let n = write_xlsx(&p, &[export_row("甲项目", Some("P-01")), export_row("乙项目", None)]).unwrap();
        assert_eq!(n, 2);
        let cells = read_cells(&p);
        assert_eq!(cells[0].join("|"), "日期|项目编号|所属项目|工作内容|时长（小时）", "表头五列");
        assert_eq!(cells.len(), 3, "表头 + 两行数据");
        assert_eq!(cells[1].join("|"), "2026-10-16|P-01|甲项目|联调第三方接口|7.5");
        assert_eq!(cells[2][1], "", "编号为 None 要落成空格，不是字面 \"None\"");
        // **按变体**钉两条刻意决定的类型（复审 Important：只比字符串这两条都打不红）：
        // 工时要是数字单元（用户在 Excel 里要能求和），日期要文本（这条环不经过序列号）。
        let typed = read_typed(&p);
        assert!(is_numeric(&typed[1][4]), "工时列必须是数字单元，实际：{:?}", typed[1][4]);
        assert!(
            matches!(typed[1][0], calamine::Data::String(_)),
            "日期列必须是文本单元，实际：{:?}",
            typed[1][0]
        );
        // 工作表名也是 spec 钉住的值（§六.1「工作表名 日报」），Task 7 只读第一个 sheet，名字错了没人报。
        assert_eq!(first_sheet_name(&p), "日报");
    }

    /// §六.1：日期写成**文本**。钉的是「导出 → 用户编辑 → 再导回来」这条环不经过 Excel 序列号。
    /// **形状分不出来**（Task 6 复审核包缓存改准了原先这句：`ExcelDateTime` 的 `Display` 印的是裸
    /// `self.value`，也就是序列号 `46311`，与 `Data::Float` 的印法一样，`datatype.rs:986-990`）——
    /// 字符串比对在这里确实能红（`2026-10-16` ≠ `46311`），但**类型要靠上面那条 `matches!(.., Data::String(_))`**。
    #[test]
    fn export_writes_the_date_as_text_not_a_date_cell() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.xlsx");
        write_xlsx(&p, &[export_row("甲项目", None)]).unwrap();
        assert_eq!(read_cells(&p)[1][0], "2026-10-16", "要读回纯日期文本，不许是日期单元的序列化形状");
    }

    /// 「下载模板」= 传空 rows：表头必须在，否则用户填无可填、导入必然报「缺列」。
    #[test]
    fn export_with_no_rows_still_writes_the_five_headers() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("template.xlsx");
        assert_eq!(write_xlsx(&p, &[]).unwrap(), 0);
        let cells = read_cells(&p);
        assert_eq!(cells.len(), 1, "模板只有表头一行");
        assert_eq!(cells[0].len(), EXPORT_HEADERS.len());
    }

    /// 写不出去按 `fs_failed` 报（`error.rs:21` 的既有码与文案），不许冒成 invalid_input 说「你输入错了」。
    #[test]
    fn write_xlsx_reports_fs_failed_when_the_directory_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("no-such-dir").join("out.xlsx");
        let err = write_xlsx(&p, &[export_row("甲项目", None)]).unwrap_err();
        assert_eq!(err.code, "fs_failed", "落点不可写是文件域，不是校验域：{}", err.message);
        assert!(err.message.contains("失败"), "{}", err.message);
    }


    /// 造一份表头 + 若干行文本/数字单元的 xlsx。**列按 `headers` 的顺序写**，
    /// 所以「插一列」「换列序」这两种用户动作都能造出来 —— M⑧ 的靶就是这里。
    fn fixture_book(rows: &[Vec<String>], headers: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("in.xlsx");
        {
            use rust_xlsxwriter::Workbook;
            let mut wb = Workbook::new();
            let ws = wb.add_worksheet();
            for (c, h) in headers.iter().enumerate() {
                ws.write(0, c as u16, *h).unwrap();
            }
            for (r, cells) in rows.iter().enumerate() {
                for (c, v) in cells.iter().enumerate() {
                    // 数字串按原样写成文本；要数字单元用 `fixture_book_typed`（下面第 5 条测试用）。
                    ws.write((r + 1) as u32, c as u16, v.as_str()).unwrap();
                }
            }
            wb.save(&p).unwrap();
        }
        dir
    }

    /// 五列都齐的完整行（日期、编号、名称、内容、工时）。
    fn full_row(line: usize) -> Vec<String> {
        vec![
            format!("2026-10-{line:02}"),
            "P-01".to_owned(),
            "甲项目".to_owned(),
            "联调第三方接口".to_owned(),
            "7.5".to_owned(),
        ]
    }

    #[test]
    fn read_rows_locates_columns_by_header_text_even_when_reordered_or_inserted() {
        // 用户把「工作内容」挪到最后一列、又在最前面插了一列「备注」⇒ 按列序硬数的实现会整表错位（M⑧）。
        let headers = ["备注", "所属项目", "日期", "时长（小时）", "工作内容", "项目编号"];
        let dir = fixture_book(&[vec!["随便".to_owned(), "甲项目".to_owned(), "2026-10-16".to_owned(), "7.5".to_owned(), "联调".to_owned(), "P-01".to_owned()], ], &headers);
        let rows = read_rows(&dir.path().join("in.xlsx")).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].line, 2, "表头是第 1 行 ⇒ 首条数据的 Excel 绝对行号是 2");
        assert_eq!(rows[0].content, Cell::Text("联调".to_owned()));
        assert_eq!(rows[0].hours, Cell::Text("7.5".to_owned()));
        assert_eq!(rows[0].code, Cell::Text("P-01".to_owned()));
    }

    #[test]
    fn read_rows_names_the_missing_header_columns() {
        let dir = fixture_book(&[], &["日期", "所属项目", "工作内容"]);
        let err = read_rows(&dir.path().join("in.xlsx")).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(err.message.contains("项目编号"), "要点名缺哪一列：{}", err.message);
        assert!(err.message.contains("时长（小时）"), "两列都要念出来：{}", err.message);
    }

    #[test]
    fn read_rows_returns_excel_absolute_line_numbers() {
        // 三行数据 ⇒ 行号 2、3、4。用户看的是 Excel，报 0-based 下标等于没报（spec §六.2）。
        let dir = fixture_book(
            &[full_row(16), full_row(17), full_row(18)],
            &EXPORT_HEADERS,
        );
        let rows = read_rows(&dir.path().join("in.xlsx")).unwrap();
        let lines: Vec<u32> = rows.iter().map(|r| r.line).collect();
        assert_eq!(lines, vec![2, 3, 4], "实际：{lines:?}");
    }

    /// **这一支是本计划原先记为「只能靠真机第 15 格」的那格，Task 6 的 M④ 变异证明本机造得出来**
    /// （`write_number` 一个序列号 + `set_num_format("yyyy\\-mm\\-dd")` 就是 Excel 手填日期的形状），
    /// 所以它回到单测里。它同时是「calamine 到底把日期样式归成哪个 `Data` 臂」的端到端追问：
    /// 若这里红在 `Cell::Number(46311.0)`，说明样式没被认出来，就要在 `to_cell` 里补序列号分支
    /// （基准 1899-12-30），并回来改 spec §三.2 那句「序列号换算是库内部的事」——
    /// 两种结果都算这条测试赢了，因为它把一个原本只能靠人眼的问题变成了一次读数。
    #[test]
    fn read_rows_recognizes_a_date_formatted_cell() {
        use rust_xlsxwriter::{Format, Workbook};
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("serial.xlsx");
        {
            let mut wb = Workbook::new();
            let ws = wb.add_worksheet();
            for (c, h) in EXPORT_HEADERS.iter().enumerate() {
                ws.write(0, c as u16, *h).unwrap();
            }
            let dfmt = Format::new().set_num_format("yyyy\\-mm\\-dd");
            ws.write_with_format(1, 0, 46311.0_f64, &dfmt).unwrap(); // 46311 = 2026-10-16 的序列号
            ws.write(1, 1, "P-01").unwrap();
            ws.write(1, 2, "甲项目").unwrap();
            ws.write(1, 3, "联调第三方接口").unwrap();
            ws.write(1, 4, "7.5").unwrap();
            wb.save(&p).unwrap();
        }
        let rows = read_rows(&p).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].date,
            Cell::DateTime(2026, 10, 16, 0, 0, 0),
            "日期样式必须走 DateTime 臂，实际：{:?}",
            rows[0].date
        );
        let (date, truncated) = date_of_cell(&rows[0].date, 2).unwrap();
        assert_eq!(date, "2026-10-16");
        assert!(!truncated, "纯日期样式（时分秒全 0）不该报截断");
    }

    #[test]
    fn date_of_cell_accepts_iso_text_and_refuses_numbers_and_blanks() {
        assert_eq!(date_of_cell(&Cell::Text(" 2026-10-16 ".to_owned()), 2).unwrap(), ("2026-10-16".to_owned(), false));
        // 形状对、历法错的值照收（与后端 is_iso_date 同口径，引历法库就破零新依赖）：
        assert!(date_of_cell(&Cell::Text("2026-13-45".to_owned()), 2).is_ok());
        for bad in [Cell::Empty, Cell::Number(20261016.0), Cell::Text("2026.10.16".to_owned()), Cell::Other] {
            let r = date_of_cell(&bad, 7).unwrap_err();
            assert_eq!(r.line, 7);
            assert_eq!(r.field, "日期");
        }
    }

    #[test]
    fn date_of_cell_truncates_the_time_part_of_a_date_cell_and_says_so() {
        let (s, truncated) = date_of_cell(&Cell::DateTime(2026, 10, 16, 8, 30, 0), 3).unwrap();
        assert_eq!(s, "2026-10-16", "日期单元取到日为止");
        assert!(truncated, "带时分秒 ⇒ 要记一行提示，不许静默截（spec §六.2）");
        let (_, clean) = date_of_cell(&Cell::DateTime(2026, 10, 16, 0, 0, 0), 3).unwrap();
        assert!(!clean, "纯日期单元不该报截断");
        let (two_digit, _) = date_of_cell(&Cell::DateTime(2026, 1, 5, 0, 0, 0), 3).unwrap();
        assert_eq!(two_digit, "2026-01-05", "月日要补零成 YYYY-MM-DD");
    }

    #[test]
    fn hours_of_cell_accepts_float_and_text_and_refuses_a_time_format_cell() {
        assert_eq!(hours_of_cell(&Cell::Number(8.5), 4).unwrap(), 8.5);
        assert_eq!(hours_of_cell(&Cell::Number(8.0), 4).unwrap(), 8.0);
        assert_eq!(hours_of_cell(&Cell::Text(" 7.53 ".to_owned()), 4).unwrap(), 7.53);
        let err = hours_of_cell(&Cell::DateTime(1899, 12, 31, 8, 30, 0), 4).unwrap_err();
        assert_eq!(err.field, "工时");
        assert!(err.reason.contains("8:30") && err.reason.contains("8.5"), "要教用户怎么改：{}", err.reason);
    }

    #[test]
    fn hours_of_cell_refuses_full_width_thousands_separators_and_out_of_range() {
        for bad in ["８", "1,5", "1e2", "abc", "", "30", "0.001"] {
            let r = hours_of_cell(&Cell::Text(bad.to_owned()), 9).unwrap_err();
            assert_eq!(r.line, 9, "「{bad}」要按行号报出来：{r:?}");
            assert_eq!(r.field, "工时");
        }
        assert!(hours_of_cell(&Cell::Number(30.0), 9).is_err(), "越界的数字单元也要拒");
    }

    fn project_rows() -> Vec<ProjectRow> {
        vec![
            ProjectRow { id: "p1".to_owned(), code: Some("P-01".to_owned()), name: "甲项目".to_owned() },
            ProjectRow { id: "p2".to_owned(), code: None, name: "乙项目".to_owned() },
            ProjectRow { id: "p3".to_owned(), code: Some("P-03".to_owned()), name: "甲项目".to_owned() },
        ]
    }

    #[test]
    fn project_of_cells_prefers_code_over_name_when_both_are_filled() {
        let ps = project_rows();
        // p1 与 p3 同名「甲项目」⇒ 只按名称会歧义，填了编号就必须按编号定。
        let id = project_of_cells(&Cell::Text("P-03".to_owned()), &Cell::Text("甲项目".to_owned()), 5, &ps).unwrap();
        assert_eq!(id, "p3", "编号优先，命中即用");
    }

    #[test]
    fn project_of_cells_falls_back_to_name_and_rejects_unknown_or_ambiguous() {
        let ps = project_rows();
        assert_eq!(project_of_cells(&Cell::Empty, &Cell::Text("乙项目".to_owned()), 5, &ps).unwrap(), "p2");
        // 编号写错但名称唯一 ⇒ 退到名称（用户可能改了编号那列）。
        assert_eq!(project_of_cells(&Cell::Text("P-99".to_owned()), &Cell::Text("乙项目".to_owned()), 5, &ps).unwrap(), "p2");
        let amb = project_of_cells(&Cell::Empty, &Cell::Text("甲项目".to_owned()), 5, &ps).unwrap_err();
        assert!(amb.reason.contains("重名 2 个") && amb.reason.contains("项目编号"), "要说清怎么办：{}", amb.reason);
        let none = project_of_cells(&Cell::Empty, &Cell::Text("没这个项目".to_owned()), 5, &ps).unwrap_err();
        assert!(none.reason.contains("没这个项目"), "要念出用户写的那个名字：{}", none.reason);
        assert_eq!(
            project_of_cells(&Cell::Empty, &Cell::Empty, 5, &ps).unwrap_err().field,
            "所属项目"
        );
    }

    /// 端到端：一份齐的行 + 一行坏日期 + 一行不存在的项目 ⇒ 预检各计其数、行号是绝对行号。
    #[test]
    fn preview_counts_ok_rejects_and_truncated_separately() {
        let conn = db::open_in_memory().unwrap();
        conn.execute("INSERT INTO projects (id, name, code, status) VALUES ('p1','甲项目','P-01','立项')", []).unwrap();
        let dir = fixture_book(
            &[
                full_row(16),
                vec!["2026/10/17".to_owned(), "P-01".to_owned(), "甲项目".to_owned(), "乙".to_owned(), "1".to_owned()],
                vec!["2026-10-18".to_owned(), "".to_owned(), "不存在".to_owned(), "丙".to_owned(), "1".to_owned()],
            ],
            &EXPORT_HEADERS,
        );
        let pv = preview(&conn, &dir.path().join("in.xlsx")).unwrap();
        assert_eq!(pv.total, 3);
        assert_eq!(pv.ok, 1, "实际：{pv:?}");
        assert_eq!(pv.rejected.len(), 2);
        assert_eq!(pv.rejected[0].line, 3, "坏日期那条在 Excel 里是第 3 行");
        assert_eq!(pv.rejected[0].field, "日期");
        assert_eq!(pv.rejected[1].line, 4);
        assert_eq!(pv.rejected[1].field, "所属项目");
        assert_eq!(pv.duplicates, 0);
    }

    #[test]
    fn preview_flags_exact_duplicates_without_blocking_them() {
        let conn = db::open_in_memory().unwrap();
        conn.execute("INSERT INTO projects (id, name, code, status) VALUES ('p1','甲项目','P-01','立项')", []).unwrap();
        create(&conn, &input("p1", Some("2026-10-16"), "联调第三方接口", 7.5)).unwrap();
        let dir = fixture_book(&[full_row(16)], &EXPORT_HEADERS);
        let pv = preview(&conn, &dir.path().join("in.xlsx")).unwrap();
        assert_eq!(pv.ok, 1, "重复不是错误，D2 之后「同一天同项目同内容同工时」合法");
        assert_eq!(pv.duplicates, 1, "但要提示，否则用户会连点两次导入按钮");
        assert_eq!(pv.duplicate_lines, vec![2]);
    }

    #[test]
    fn preview_refuses_more_than_max_import_rows() {
        let conn = db::open_in_memory().unwrap();
        conn.execute("INSERT INTO projects (id, name, code, status) VALUES ('p1','甲项目','P-01','立项')", []).unwrap();
        let rows: Vec<Vec<String>> = (0..=MAX_IMPORT_ROWS).map(|i| full_row(i % 28 + 1)).collect();
        let dir = fixture_book(&rows, &EXPORT_HEADERS);
        let err = preview(&conn, &dir.path().join("in.xlsx")).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(err.message.contains(&MAX_IMPORT_ROWS.to_string()), "要念出上限：{}", err.message);
    }

    /// **M⑦ 的靶子**：把「先全量校验、后一次性写入」改成「逐行校验、逐行提交」，这条当场红
    /// （红在 `left: 1 / right: 0`，即坏行之前的那条已经落了库）。
    /// **别读成「拆掉 `transaction()` 就会红」** —— Task 7 实测拆了也不红：坏行在 `validate_row`
    /// 那一步就被拦下，根本走不到写。事务防的是写入阶段自身的失败（约束、磁盘、掉电），
    /// 是第二道防线而不是唯一防线；它的提交侧由 `apply_rereads_the_file_and_reports_the_inserted_count` 守着。
    #[test]
    fn apply_inserts_all_or_nothing() {
        let conn = db::open_in_memory().unwrap();
        conn.execute("INSERT INTO projects (id, name, code, status) VALUES ('p1','甲项目','P-01','立项')", []).unwrap();
        let dir = fixture_book(
            &[full_row(16), vec!["2026-10-17".to_owned(), "P-01".to_owned(), "甲项目".to_owned(), "乙".to_owned(), "30".to_owned()]],
            &EXPORT_HEADERS,
        );
        let mut conn = conn;
        let err = apply(&mut conn, &dir.path().join("in.xlsx")).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(err.message.contains("第 3 行"), "整批回滚也要说清坏在哪一行：{}", err.message);
        assert!(err.message.contains("工时"), "{}", err.message);
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM daily_reports", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "任一行不合格 ⇒ 库里一条都不许多");
    }

    #[test]
    fn apply_rereads_the_file_and_reports_the_inserted_count() {
        let conn = db::open_in_memory().unwrap();
        conn.execute("INSERT INTO projects (id, name, code, status) VALUES ('p1','甲项目','P-01','立项')", []).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("in.xlsx");
        std::fs::copy(fixture_book(&[full_row(16), full_row(17)], &EXPORT_HEADERS).path().join("in.xlsx"), &p).unwrap();
        let mut conn = conn;
        let out = apply(&mut conn, &p).unwrap();
        assert_eq!(out.inserted, 2);
        assert_eq!(list(&conn, None).unwrap().len(), 2);
        assert_eq!(list(&conn, None).unwrap()[0].project_id, "p1", "按编号匹配回来的项目 id 要落进行");
    }

    #[test]
    fn apply_refuses_a_header_only_file() {
        let conn = db::open_in_memory().unwrap();
        let dir = fixture_book(&[], &EXPORT_HEADERS);
        let mut conn = conn;
        let err = apply(&mut conn, &dir.path().join("in.xlsx")).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(err.message.contains("没有可导入的行"), "{}", err.message);
    }

    #[test]
    fn import_preview_line_format_has_exactly_six_keys() {
        // 期望数组按 `sort_unstable()` 的字节序写：`duplicateLines` 那一格是大写 `L`（U+004C）、
        // `duplicates` 是小写 `s`（U+0073），所以前者在前 —— 不是忽略大小写的字母表顺序（简报那行按
        // 后者写，跑出来是恒红，按读数改准）。钉的键集合一个字没动：少一个、多一个、改名都照样红。
        let pv = ImportPreview {
            total: 0,
            ok: 0,
            duplicates: 0,
            duplicate_lines: vec![],
            truncated_lines: vec![],
            rejected: vec![],
        };
        let obj = serde_json::to_value(&pv).unwrap();
        let mut keys: Vec<&str> = obj.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec!["duplicateLines", "duplicates", "ok", "rejected", "total", "truncatedLines"],
            "ImportPreview 的键集合变了就要同步 src/types/daily-report.ts"
        );
    }

    #[test]
    fn import_outcome_line_format_has_exactly_one_key() {
        let obj = serde_json::to_value(ImportOutcome { inserted: 3 }).unwrap();
        assert_eq!(obj.as_object().unwrap().keys().collect::<Vec<_>>(), vec!["inserted"]);
    }
}
