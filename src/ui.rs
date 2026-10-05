pub fn message(text: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            crate::platform::wide(text).as_ptr(),
            crate::platform::wide("RoLauncher").as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}
use crate::{
    engine::{AccountPatch, Engine},
    model::{Account, Target},
    platform::wide,
};
use std::ptr::{null, null_mut};
use std::{cell::RefCell, collections::HashMap};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Controls::Dialogs::*, Controls::*, Input::KeyboardAndMouse::*, Shell::*,
        WindowsAndMessaging::*,
    },
};

const IMPORT: u16 = 101;
const FILE_IMPORT: u16 = 102;
const LOGIN: u16 = 103;
const REJOIN: u16 = 205;
const TABS: u16 = 211;
const START: u16 = 105;
const STOP: u16 = 106;
const RESTART: u16 = 107;
const REMOVE: u16 = 108;
const TOKEN: u16 = 109;
const EXIT: u16 = 110;
const DISCORD_ENABLE: u16 = 215;
const DISCORD_RECOVERY: u16 = 216;
const DISCORD_URL: u16 = 217;
const DISCORD_SAVE: u16 = 218;
const DISCORD_TEST: u16 = 219;
const DISCORD_REMOVE: u16 = 220;
const TRAY_MSG: u32 = WM_APP + 1;
const CHANGELOG: &str = include_str!("../desktop/CHANGELOG.txt");
struct Ui {
    engine: Engine,
    runtime: tokio::runtime::Handle,
    port: u16,
    list: HWND,
    cookie: HWND,
    alias: HWND,
    place: HWND,
    job: HWND,
    private: HWND,
    rejoin: HWND,
    status: HWND,
    accounts: Vec<Account>,
    last: String,
    refreshing: bool,
    tabs: HWND,
    pages: [Vec<HWND>; 3],
    page: usize,
    settings_summary: HWND,
    changelog: HWND,
    editor_id: Option<String>,
    drafts: HashMap<String, Draft>,
    discord_url: HWND,
    discord_enabled: HWND,
    discord_recovery: HWND,
    discord_status: HWND,
    activity: HWND,
    last_discord: String,
}
#[derive(Clone, Default)]
struct Draft {
    alias: String,
    place: String,
    job: String,
    private: String,
    alias_dirty: bool,
    target_dirty: bool,
}
impl Draft {
    fn from_account(account: &Account) -> Self {
        Self {
            alias: account.alias.clone(),
            place: account
                .target
                .as_ref()
                .map(|t| t.place_id.to_string())
                .unwrap_or_default(),
            job: account
                .target
                .as_ref()
                .and_then(|t| t.job_id)
                .map(|id| id.to_string())
                .unwrap_or_default(),
            private: account
                .target
                .as_ref()
                .and_then(|t| t.private_server_link.clone())
                .unwrap_or_default(),
            ..Default::default()
        }
    }
    fn target(&self) -> Result<Target, String> {
        let target = Target {
            place_id: self
                .place
                .trim()
                .parse()
                .map_err(|_| "Enter a numeric PlaceId")?,
            job_id: if self.job.trim().is_empty() {
                None
            } else {
                Some(
                    self.job
                        .trim()
                        .parse()
                        .map_err(|_| "JobId must be a UUID")?,
                )
            },
            private_server_link: if self.private.trim().is_empty() {
                None
            } else {
                Some(self.private.trim().into())
            },
        };
        target.validate()?;
        Ok(target)
    }
}
fn text(hwnd: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        let mut b = vec![0u16; len as usize + 1];
        let n = GetWindowTextW(hwnd, b.as_mut_ptr(), b.len() as i32);
        String::from_utf16_lossy(&b[..n as usize])
    }
}
fn set(hwnd: HWND, value: &str) {
    unsafe {
        SetWindowTextW(hwnd, wide(value).as_ptr());
    }
}
#[allow(clippy::too_many_arguments)] // Mirrors the native Win32 control creation parameters.
fn control(
    parent: HWND,
    class: &str,
    label: &str,
    id: u16,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    style: u32,
) -> HWND {
    unsafe {
        let child = CreateWindowExW(
            0,
            wide(class).as_ptr(),
            wide(label).as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            x,
            y,
            w,
            h,
            parent,
            id as usize as HMENU,
            GetModuleHandleW(null()),
            null(),
        );
        SendMessageW(
            child,
            WM_SETFONT,
            GetStockObject(DEFAULT_GUI_FONT) as usize,
            1,
        );
        child
    }
}
fn button(parent: HWND, label: &str, id: u16, x: i32, y: i32, w: i32) {
    control(
        parent,
        "BUTTON",
        label,
        id,
        x,
        y,
        w,
        28,
        WS_TABSTOP | BS_PUSHBUTTON as u32,
    );
}
impl Ui {
    fn refresh_discord(&mut self) {
        let view = self.engine.discord_settings();
        let serialized = serde_json::to_string(&view).unwrap_or_default();
        if serialized == self.last_discord {
            return;
        }
        self.last_discord = serialized;
        unsafe {
            SendMessageW(
                self.discord_enabled,
                BM_SETCHECK,
                if view.enabled {
                    BST_CHECKED
                } else {
                    BST_UNCHECKED
                } as usize,
                0,
            );
            SendMessageW(
                self.discord_recovery,
                BM_SETCHECK,
                if view.notify_recovery {
                    BST_CHECKED
                } else {
                    BST_UNCHECKED
                } as usize,
                0,
            );
        }
        set(
            self.discord_status,
            &format!(
                "{} {}",
                if view.configured {
                    if view.enabled {
                        "Notifications on."
                    } else {
                        "Webhook saved; notifications off."
                    }
                } else {
                    "No webhook saved."
                },
                view.delivery_status
            ),
        );
        let activity = view
            .recent
            .iter()
            .map(|notice| {
                format!(
                    "{} | {} | {}\r\n{}\r\n",
                    notice
                        .timestamp
                        .with_timezone(&chrono::Local)
                        .format("%Y-%m-%d %H:%M:%S"),
                    notice.account,
                    notice.title,
                    notice.message
                )
            })
            .collect::<Vec<_>>()
            .join("\r\n");
        set(
            self.activity,
            if activity.is_empty() {
                "Account kicks, disconnects, client exits, rejoin attempts and attention alerts will appear here."
            } else {
                &activity
            },
        );
    }
    fn save_discord(&mut self) -> bool {
        let url = zeroize::Zeroizing::new(text(self.discord_url));
        let result = self.engine.set_discord(crate::discord::DiscordPatch {
            enabled: Some(
                unsafe { SendMessageW(self.discord_enabled, BM_GETCHECK, 0, 0) }
                    == BST_CHECKED as isize,
            ),
            notify_recovery: Some(
                unsafe { SendMessageW(self.discord_recovery, BM_GETCHECK, 0, 0) }
                    == BST_CHECKED as isize,
            ),
            webhook_url: if url.trim().is_empty() {
                None
            } else {
                Some(url.trim().into())
            },
        });
        match result {
            Ok(_) => {
                set(self.discord_url, "");
                true
            }
            Err(error) => {
                message(&error);
                self.last_discord.clear();
                self.refresh_discord();
                false
            }
        }
    }
    #[cfg(test)]
    fn smoke_actions(&mut self, hwnd: HWND) {
        assert_eq!(self.accounts.len(), 3);
        assert!(unsafe { GetDlgItem(hwnd, 104) }.is_null()); // No Save button.
        assert_eq!(
            unsafe { SendMessageW(self.tabs, TCM_GETITEMCOUNT, 0, 0) },
            3
        );
        let mut selection: LVITEMW = unsafe { std::mem::zeroed() };
        selection.stateMask = LVIS_SELECTED;
        selection.state = LVIS_SELECTED;
        unsafe {
            SendMessageW(
                self.list,
                LVM_SETITEMSTATE,
                0,
                &selection as *const _ as isize,
            );
        }
        self.select_fields();
        let id = self.selected_ids()[0].clone();
        set(self.alias, "Renamed");
        self.edit_changed(hwnd, 202);
        self.save_drafts();
        assert_eq!(
            self.engine
                .snapshot()
                .accounts
                .iter()
                .find(|a| a.id == id)
                .unwrap()
                .alias,
            "Renamed"
        );
        set(self.place, "incomplete");
        self.edit_changed(hwnd, 203);
        self.save_drafts();
        assert_eq!(
            self.engine
                .snapshot()
                .accounts
                .iter()
                .find(|a| a.id == id)
                .unwrap()
                .target
                .as_ref()
                .unwrap()
                .place_id,
            1
        );
        self.switch_page(1);
        set(
            self.discord_url,
            "https://discord.com/api/webhooks/123456/TEST_WEBHOOK",
        );
        self.action(hwnd, DISCORD_SAVE);
        assert!(self.engine.discord_settings().configured);
        assert!(text(self.discord_url).is_empty());
        assert!(
            unsafe { GetWindowLongPtrW(self.discord_url, GWL_STYLE) } as u32 & ES_PASSWORD as u32
                != 0
        );
        unsafe {
            SendMessageW(self.discord_enabled, BM_SETCHECK, BST_CHECKED as usize, 0);
        }
        self.action(hwnd, DISCORD_ENABLE);
        assert!(self.engine.discord_settings().enabled);
        unsafe {
            SendMessageW(
                self.discord_recovery,
                BM_SETCHECK,
                BST_UNCHECKED as usize,
                0,
            );
        }
        self.action(hwnd, DISCORD_RECOVERY);
        assert!(!self.engine.discord_settings().notify_recovery);
        self.action(hwnd, DISCORD_TEST); // Queue only; this fixture never spawns the sender.
        assert!(text(self.discord_status).contains("queued"));
        assert!(text(self.activity).contains("Discord notifications are ready"));
        self.action(hwnd, DISCORD_REMOVE);
        assert!(!self.engine.discord_settings().configured);
        assert!(!self.engine.discord_settings().enabled);
        self.switch_page(0);
        self.select_fields();
        assert_eq!(text(self.place), "incomplete");
        set(self.place, "1234");
        self.edit_changed(hwnd, 203);
        self.save_drafts();
        assert_eq!(
            self.engine
                .snapshot()
                .accounts
                .iter()
                .find(|a| a.id == id)
                .unwrap()
                .target
                .as_ref()
                .unwrap()
                .place_id,
            1234
        );
        self.refresh();
        self.select_all();
        assert_eq!(self.selected_ids().len(), 3);
        assert_eq!(unsafe { IsWindowEnabled(self.alias) }, 0);
        self.switch_page(1);
        unsafe {
            SendMessageW(self.rejoin, BM_SETCHECK, BST_UNCHECKED as usize, 0);
        }
        self.action(hwnd, REJOIN);
        assert!(
            self.engine
                .snapshot()
                .accounts
                .iter()
                .all(|a| !a.auto_recovery)
        );
        self.engine
            .patch(
                &id,
                AccountPatch {
                    alias: None,
                    target: None,
                    auto_recovery: Some(true),
                    ..Default::default()
                },
            )
            .unwrap();
        self.refresh();
        assert_eq!(
            unsafe { SendMessageW(self.rejoin, BM_GETCHECK, 0, 0) },
            BST_INDETERMINATE as isize
        );
        unsafe {
            SendMessageW(self.rejoin, BM_SETCHECK, BST_CHECKED as usize, 0);
        }
        self.action(hwnd, REJOIN);
        assert!(
            self.engine
                .snapshot()
                .accounts
                .iter()
                .all(|a| a.auto_recovery)
        );
        self.switch_page(2);
        assert!(!text(self.changelog).is_empty());
        assert!(
            self.pages[0]
                .iter()
                .all(|h| unsafe { GetWindowLongPtrW(*h, GWL_STYLE) } as u32 & WS_VISIBLE == 0)
        );
        self.switch_page(0);
        self.action(hwnd, START);
        assert!(
            self.engine
                .snapshot()
                .accounts
                .iter()
                .all(|a| a.desired_running)
        );
        assert_eq!(self.selected_ids().len(), 3);
        self.action(hwnd, STOP);
        assert!(
            self.engine
                .snapshot()
                .accounts
                .iter()
                .all(|a| !a.desired_running)
        );
        self.action(hwnd, RESTART);
        assert!(
            self.engine
                .snapshot()
                .accounts
                .iter()
                .all(|a| a.desired_running)
        );
        self.action(hwnd, STOP);
        self.action(hwnd, REMOVE);
        assert!(self.engine.snapshot().accounts.is_empty());
    }
    fn select_all(&mut self) {
        self.save_drafts();
        let mut item: LVITEMW = unsafe { std::mem::zeroed() };
        item.stateMask = LVIS_SELECTED;
        item.state = LVIS_SELECTED;
        unsafe {
            SendMessageW(
                self.list,
                LVM_SETITEMSTATE,
                usize::MAX,
                &item as *const _ as isize,
            );
        }
        self.select_fields();
    }
    fn selected_ids(&self) -> Vec<String> {
        let mut ids = Vec::new();
        let mut index = -1isize;
        loop {
            index = unsafe {
                SendMessageW(
                    self.list,
                    LVM_GETNEXTITEM,
                    index as usize,
                    LVNI_SELECTED as isize,
                )
            };
            if index < 0 {
                break;
            }
            if let Some(account) = self.accounts.get(index as usize) {
                ids.push(account.id.clone());
            }
        }
        ids
    }
    fn select_fields(&mut self) {
        self.refreshing = true;
        let ids = self.selected_ids();
        let snapshot = self.engine.snapshot();
        self.editor_id = if ids.len() == 1 {
            Some(ids[0].clone())
        } else {
            None
        };
        let selected = self
            .editor_id
            .as_ref()
            .and_then(|id| snapshot.accounts.iter().find(|a| &a.id == id));
        let draft = selected.map(|account| {
            self.drafts
                .get(&account.id)
                .cloned()
                .unwrap_or_else(|| Draft::from_account(account))
        });
        let fields = draft.unwrap_or_default();
        set(self.alias, &fields.alias);
        set(self.place, &fields.place);
        set(self.job, &fields.job);
        set(self.private, &fields.private);
        for field in [self.alias, self.place, self.job, self.private] {
            unsafe {
                EnableWindow(field, i32::from(ids.len() == 1));
            }
        }
        let enabled = ids
            .iter()
            .filter(|id| {
                snapshot
                    .accounts
                    .iter()
                    .any(|a| a.id == **id && a.auto_recovery)
            })
            .count();
        let check = if enabled == 0 {
            BST_UNCHECKED
        } else if enabled == ids.len() {
            BST_CHECKED
        } else {
            BST_INDETERMINATE
        };
        unsafe {
            EnableWindow(self.rejoin, i32::from(!ids.is_empty()));
            SendMessageW(self.rejoin, BM_SETCHECK, check as usize, 0);
        }
        set(
            self.settings_summary,
            &format!(
                "{} account(s) selected. Automatic rejoin is on for {}. Select accounts in the Accounts tab; this switch applies to the selection.",
                ids.len(),
                enabled
            ),
        );
        if ids.len() > 1 {
            set(
                self.status,
                &format!(
                    "{} accounts selected. Start, Stop, Restart and Remove apply to all selected accounts. Edit individual targets by selecting one account.",
                    ids.len()
                ),
            );
        } else if let Some(account) = selected {
            set(
                self.status,
                &account
                    .last_error
                    .clone()
                    .map(|error| {
                        error
                            .replace("recovery", "rejoin")
                            .replace("Recovery", "Rejoin")
                    })
                    .unwrap_or_else(|| format!("{}: {:?}", account.username, account.status)),
            );
        } else {
            set(
                self.status,
                "Select one account to edit its target, or Ctrl+A in the account table to select all.",
            );
        }
        self.refreshing = false;
    }
    fn edit_changed(&mut self, hwnd: HWND, id: u16) {
        if self.refreshing {
            return;
        }
        let Some(account_id) = self.editor_id.clone() else {
            return;
        };
        let draft = self.drafts.entry(account_id).or_default();
        draft.alias = text(self.alias);
        draft.place = text(self.place);
        draft.job = text(self.job);
        draft.private = text(self.private);
        if id == 202 {
            draft.alias_dirty = true;
        } else {
            draft.target_dirty = true;
        }
        set(self.status, "Saving changes automatically…");
        unsafe {
            SetTimer(hwnd, 3, 600, None);
        }
    }
    fn save_drafts(&mut self) {
        let ids: Vec<_> = self.drafts.keys().cloned().collect();
        for id in ids {
            let Some(mut draft) = self.drafts.get(&id).cloned() else {
                continue;
            };
            let target = if draft.target_dirty {
                draft.target().map(Some)
            } else {
                Ok(None)
            };
            // Valid aliases still save while an incomplete destination stays in the editor.
            let patch = AccountPatch {
                alias: draft.alias_dirty.then(|| draft.alias.clone()),
                target: target.clone().ok().flatten(),
                auto_recovery: None,
                ..Default::default()
            };
            if patch.alias.is_some() || patch.target.is_some() {
                match self.engine.patch(&id, patch) {
                    Ok(_) => {
                        draft.alias_dirty = false;
                        if target.is_ok() {
                            draft.target_dirty = false;
                        }
                    }
                    Err(error) => {
                        set(self.status, &format!("Changes not saved: {error}"));
                        self.drafts.insert(id, draft);
                        continue;
                    }
                }
            }
            if let Err(error) = target {
                set(
                    self.status,
                    &format!(
                        "Destination not saved: {error}. Complete the fields; the previous destination is retained."
                    ),
                );
            } else {
                set(self.status, "Changes saved automatically.");
            }
            if draft.alias_dirty || draft.target_dirty {
                self.drafts.insert(id, draft);
            } else {
                self.drafts.remove(&id);
            }
        }
    }
    fn switch_page(&mut self, page: usize) {
        self.save_drafts();
        self.page = page.min(2);
        for (index, controls) in self.pages.iter().enumerate() {
            for control in controls {
                unsafe {
                    ShowWindow(*control, if index == self.page { SW_SHOW } else { SW_HIDE });
                }
            }
        }
        if self.page == 1 {
            self.select_fields();
        }
    }
    fn refresh(&mut self) {
        self.refresh_discord();
        let snapshot = self.engine.snapshot();
        let serialized = serde_json::to_string(&snapshot).unwrap_or_default();
        if serialized == self.last {
            return;
        }
        let selection = self.selected_ids();
        self.refreshing = true;
        unsafe {
            SendMessageW(self.list, WM_SETREDRAW, 0, 0);
            SendMessageW(self.list, LVM_DELETEALLITEMS, 0, 0);
        }
        self.accounts = snapshot.accounts;
        for (row, a) in self.accounts.iter().enumerate() {
            let target = a
                .target
                .as_ref()
                .map(|t| {
                    format!(
                        "{}{}",
                        t.place_id,
                        if t.private_server_link.is_some() {
                            " (private)"
                        } else if t.job_id.is_some() {
                            " (server)"
                        } else {
                            ""
                        }
                    )
                })
                .unwrap_or_else(|| "Set target".into());
            let values = [
                format!("{} ({})", a.alias, a.username),
                a.process
                    .as_ref()
                    .map(|p| p.pid.to_string())
                    .unwrap_or_else(|| "—".into()),
                format!("{:?}", a.status),
                target,
                a.failures.to_string(),
                if a.auto_recovery {
                    "On".into()
                } else {
                    "Off".into()
                },
                a.last_error
                    .as_deref()
                    .unwrap_or_default()
                    .replace("recovery", "rejoin")
                    .replace("Recovery", "Rejoin"),
            ];
            for (column, value) in values.iter().enumerate() {
                let mut label = wide(value);
                let mut item: LVITEMW = unsafe { std::mem::zeroed() };
                item.mask = LVIF_TEXT;
                item.iItem = row as i32;
                item.iSubItem = column as i32;
                item.pszText = label.as_mut_ptr();
                unsafe {
                    SendMessageW(
                        self.list,
                        if column == 0 {
                            LVM_INSERTITEMW
                        } else {
                            LVM_SETITEMW
                        },
                        0,
                        &item as *const _ as isize,
                    );
                }
            }
            if selection.contains(&a.id) {
                let mut item: LVITEMW = unsafe { std::mem::zeroed() };
                item.stateMask = LVIS_SELECTED;
                item.state = LVIS_SELECTED;
                unsafe {
                    SendMessageW(self.list, LVM_SETITEMSTATE, row, &item as *const _ as isize);
                }
            }
        }
        unsafe {
            SendMessageW(self.list, WM_SETREDRAW, 1, 0);
            InvalidateRect(self.list, null(), 1);
        }
        self.last = serialized;
        self.refreshing = false;
        if self.page == 1 {
            self.select_fields();
        }
        if snapshot.network_suspended {
            set(
                self.status,
                "Roblox connectivity unavailable; rejoin is suspended",
            );
        } else if self.selected_ids().is_empty() {
            set(self.status, &snapshot.compatibility);
        }
    }
    fn import_values(&self, values: zeroize::Zeroizing<String>) {
        let engine = self.engine.clone();
        self.runtime.spawn(async move {
            let mut success = 0;
            let mut errors = Vec::new();
            let lines: Vec<_> = values.lines().filter(|s| !s.trim().is_empty()).collect();
            if lines.len() > 50 {
                message("Import at most 50 cookies at once");
                return;
            }
            for (index, cookie) in lines.into_iter().enumerate() {
                match engine.import(cookie).await {
                    Ok(_) => success += 1,
                    Err(e) => errors.push(format!("Line {}: {}", index + 1, e)),
                }
            }
            message(&format!(
                "Imported {success} account(s).{}",
                if errors.is_empty() {
                    String::new()
                } else {
                    format!("\n{}", errors.join("\n"))
                }
            ));
        });
    }
    fn action(&mut self, hwnd: HWND, id: u16) {
        match id {
            DISCORD_ENABLE | DISCORD_RECOVERY | DISCORD_SAVE | DISCORD_TEST => {
                if !self.save_discord() {
                    return;
                }
                if id == DISCORD_TEST
                    && let Err(error) = self.engine.test_discord()
                {
                    message(&error);
                }
                self.refresh_discord();
            }
            DISCORD_REMOVE => {
                let result = self.engine.set_discord(crate::discord::DiscordPatch {
                    enabled: Some(false),
                    webhook_url: Some(String::new()),
                    notify_recovery: None,
                });
                if let Err(error) = result {
                    message(&error);
                }
                set(self.discord_url, "");
                self.refresh_discord();
            }
            IMPORT => {
                let cookie = zeroize::Zeroizing::new(text(self.cookie));
                set(self.cookie, "");
                if !cookie.trim().is_empty() {
                    self.import_values(cookie);
                }
            }
            FILE_IMPORT => {
                if let Some(path) = pick_file(hwnd) {
                    match std::fs::read_to_string(path) {
                        Ok(s) => self.import_values(zeroize::Zeroizing::new(s)),
                        Err(_) => message("Unable to read the cookie file"),
                    }
                }
            }
            LOGIN => {
                let engine = self.engine.clone();
                let runtime = self.runtime.clone();
                std::thread::spawn(move || {
                    if let Err(e) = crate::login::run(engine, runtime) {
                        message(&e);
                    }
                });
            }
            TOKEN => message(&format!(
                "Local API: http://127.0.0.1:{}/v1\n\nBearer token (keep private):\n{}",
                self.port,
                self.engine.token()
            )),
            EXIT => unsafe {
                self.save_drafts();
                DestroyWindow(hwnd);
            },
            REJOIN | START | STOP | RESTART | REMOVE => {
                self.save_drafts();
                let ids = self.selected_ids();
                if ids.is_empty() {
                    set(self.status, "Select at least one account first.");
                    return;
                }
                let enabled =
                    unsafe { SendMessageW(self.rejoin, BM_GETCHECK, 0, 0) } == BST_CHECKED as isize;
                let mut success = 0;
                let mut errors = Vec::new();
                for account_id in &ids {
                    let incomplete = self.drafts.get(account_id).is_some_and(|d| d.target_dirty);
                    let result = if incomplete && matches!(id, START | RESTART) {
                        Err("Complete the unsaved destination before starting this account".into())
                    } else {
                        match id {
                            REJOIN => self
                                .engine
                                .patch(
                                    account_id,
                                    AccountPatch {
                                        alias: None,
                                        target: None,
                                        auto_recovery: Some(enabled),
                                        ..Default::default()
                                    },
                                )
                                .map(|_| ()),
                            REMOVE => self.engine.remove(account_id),
                            _ => self
                                .engine
                                .command(
                                    account_id,
                                    match id {
                                        START => "start",
                                        STOP => "stop",
                                        _ => "restart",
                                    },
                                )
                                .map(|_| ()),
                        }
                    };
                    match result {
                        Ok(()) => {
                            success += 1;
                            if id == REMOVE {
                                self.drafts.remove(account_id);
                            }
                        }
                        Err(error) => {
                            let name = self
                                .accounts
                                .iter()
                                .find(|a| &a.id == account_id)
                                .map(|a| a.alias.as_str())
                                .unwrap_or(account_id);
                            errors.push(format!("{name}: {error}"));
                        }
                    }
                }
                self.refresh();
                self.select_fields();
                set(
                    self.status,
                    &format!(
                        "Applied to {success}/{} selected account(s). {}",
                        ids.len(),
                        errors.join("; ")
                    ),
                );
            }
            _ => {}
        }
    }
}
fn pick_file(hwnd: HWND) -> Option<std::path::PathBuf> {
    let mut file = vec![0u16; 32768];
    let filter: Vec<u16> = "Text files\0*.txt\0All files\0*.*\0\0"
        .encode_utf16()
        .collect();
    let mut dialog: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    dialog.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    dialog.hwndOwner = hwnd;
    dialog.lpstrFilter = filter.as_ptr();
    dialog.lpstrFile = file.as_mut_ptr();
    dialog.nMaxFile = file.len() as u32;
    dialog.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
    if unsafe { GetOpenFileNameW(&mut dialog) } == 0 {
        return None;
    }
    let end = file.iter().position(|c| *c == 0).unwrap_or(file.len());
    Some(String::from_utf16_lossy(&file[..end]).into())
}
unsafe extern "system" fn procedure(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_DESTROY {
        let mut tray: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        tray.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        tray.hWnd = hwnd;
        tray.uID = 1;
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &tray);
            KillTimer(hwnd, 1);
            KillTimer(hwnd, 3);
            PostQuitMessage(0);
        }
        return 0;
    }
    if msg == WM_NCCREATE {
        let cs = unsafe { &*(l as *const CREATESTRUCTW) };
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
        }
    }
    let p = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut RefCell<Ui>;
    if !p.is_null() {
        let cell = unsafe { &*p };
        let Ok(mut borrowed) = cell.try_borrow_mut() else {
            return unsafe { DefWindowProcW(hwnd, msg, w, l) };
        };
        let ui = &mut *borrowed;
        match msg {
            WM_COMMAND => {
                let id = (w & 0xffff) as u16;
                let notification = (w >> 16) as u32;
                if notification == EN_CHANGE && matches!(id, 202 | 203 | 204 | 206) {
                    ui.edit_changed(hwnd, id);
                } else if notification == BN_CLICKED {
                    ui.action(hwnd, id);
                }
                return 0;
            }
            WM_TIMER => {
                if w == 3 {
                    unsafe {
                        KillTimer(hwnd, 3);
                    }
                    ui.save_drafts();
                    ui.refresh();
                    return 0;
                }
                if w == 2 {
                    unsafe {
                        DestroyWindow(hwnd);
                    }
                    return 0;
                }
                ui.refresh();
                return 0;
            }
            WM_NOTIFY => {
                let hdr = unsafe { &*(l as *const NMHDR) };
                if hdr.hwndFrom == ui.tabs && hdr.code == TCN_SELCHANGE {
                    let page = unsafe { SendMessageW(ui.tabs, TCM_GETCURSEL, 0, 0) };
                    ui.switch_page(page.max(0) as usize);
                    return 0;
                }
                if !ui.refreshing && !ui.list.is_null() {
                    let hdr = unsafe { &*(l as *const NMHDR) };
                    if hdr.hwndFrom == ui.list
                        && (hdr.code == LVN_ITEMCHANGED || hdr.code == NM_CLICK)
                    {
                        ui.save_drafts();
                        ui.select_fields();
                    }
                }
            }
            WM_SIZE => {
                let width = (l as u32 & 0xffff) as i32;
                let height = ((l as u32 >> 16) & 0xffff) as i32;
                if !ui.list.is_null() {
                    unsafe {
                        MoveWindow(
                            ui.list,
                            12,
                            256,
                            (width - 24).max(200),
                            (height - 306).max(80),
                            1,
                        );
                        MoveWindow(ui.status, 12, height - 38, width - 24, 28, 1);
                        MoveWindow(ui.tabs, 12, 8, (width - 115).max(200), 30, 1);
                        MoveWindow(
                            ui.changelog,
                            12,
                            52,
                            (width - 24).max(200),
                            (height - 104).max(80),
                            1,
                        );
                        MoveWindow(
                            ui.activity,
                            12,
                            458,
                            (width - 24).max(200),
                            (height - 510).max(80),
                            1,
                        );
                    }
                }
            }
            WM_GETMINMAXINFO => {
                let limits = unsafe { &mut *(l as *mut MINMAXINFO) };
                limits.ptMinTrackSize = POINT { x: 1100, y: 720 };
                return 0;
            }
            WM_CLOSE => {
                ui.save_drafts();
                unsafe {
                    ShowWindow(hwnd, SW_HIDE);
                }
                return 0;
            }
            TRAY_MSG => {
                if l as u32 == WM_LBUTTONDBLCLK {
                    unsafe {
                        ShowWindow(hwnd, SW_RESTORE);
                        SetForegroundWindow(hwnd);
                    }
                } else if l as u32 == WM_RBUTTONUP {
                    let menu = unsafe { CreatePopupMenu() };
                    unsafe {
                        AppendMenuW(menu, MF_STRING, 1, wide("Open RoLauncher").as_ptr());
                        AppendMenuW(
                            menu,
                            MF_STRING,
                            2,
                            wide("Exit (leave clients running)").as_ptr(),
                        );
                        let mut point = std::mem::zeroed();
                        GetCursorPos(&mut point);
                        SetForegroundWindow(hwnd);
                        let command = TrackPopupMenu(
                            menu,
                            TPM_RETURNCMD | TPM_NONOTIFY,
                            point.x,
                            point.y,
                            0,
                            hwnd,
                            null(),
                        );
                        DestroyMenu(menu);
                        if command == 1 {
                            ShowWindow(hwnd, SW_RESTORE);
                        } else if command == 2 {
                            ui.save_drafts();
                            DestroyWindow(hwnd);
                        }
                    }
                }
                return 0;
            }
            _ => {}
        }
    }
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}
pub fn run(engine: Engine, runtime: tokio::runtime::Handle, port: u16) -> Result<(), String> {
    crate::desktop::run(engine, runtime, port)
}
// Retained for the existing hidden native-control regression suite.
pub fn run_native(
    engine: Engine,
    runtime: tokio::runtime::Handle,
    port: u16,
) -> Result<(), String> {
    run_window(engine, runtime, port, false)
}
fn run_window(
    engine: Engine,
    runtime: tokio::runtime::Handle,
    port: u16,
    smoke: bool,
) -> Result<(), String> {
    unsafe {
        let init = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_LISTVIEW_CLASSES | ICC_TAB_CLASSES,
        };
        InitCommonControlsEx(&init);
        let class = wide("RoLauncherWindow");
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.lpfnWndProc = Some(procedure);
        wc.hInstance = GetModuleHandleW(null());
        wc.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
        wc.hIcon = LoadIconW(null_mut(), IDI_APPLICATION);
        wc.hbrBackground = (COLOR_WINDOW + 1) as HBRUSH;
        wc.lpszClassName = class.as_ptr();
        RegisterClassW(&wc);
        let state = Box::new(RefCell::new(Ui {
            engine,
            runtime,
            port,
            list: null_mut(),
            cookie: null_mut(),
            alias: null_mut(),
            place: null_mut(),
            job: null_mut(),
            private: null_mut(),
            rejoin: null_mut(),
            status: null_mut(),
            accounts: Vec::new(),
            last: String::new(),
            refreshing: false,
            tabs: null_mut(),
            pages: Default::default(),
            page: 0,
            settings_summary: null_mut(),
            changelog: null_mut(),
            editor_id: None,
            drafts: HashMap::new(),
            discord_url: null_mut(),
            discord_enabled: null_mut(),
            discord_recovery: null_mut(),
            discord_status: null_mut(),
            activity: null_mut(),
            last_discord: String::new(),
        }));
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide(concat!(
                "RoLauncher ",
                env!("CARGO_PKG_VERSION"),
                " — Roblox accounts"
            ))
            .as_ptr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1120,
            720,
            null_mut(),
            null_mut(),
            wc.hInstance,
            &*state as *const RefCell<Ui> as *const _,
        );
        if hwnd.is_null() {
            return Err("Unable to create native window".into());
        }
        let mut state = state.borrow_mut();
        control(
            hwnd,
            "STATIC",
            "Add an account using your session cookie or browser sign-in",
            0,
            12,
            12,
            660,
            20,
            0,
        );
        state.cookie = control(
            hwnd,
            "EDIT",
            "",
            201,
            12,
            38,
            560,
            26,
            WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32 | ES_PASSWORD as u32,
        );
        SendMessageW(state.cookie, EM_SETLIMITTEXT, 16384, 0);
        button(hwnd, "Import cookie", IMPORT, 584, 38, 112);
        button(hwnd, "Import file…", FILE_IMPORT, 706, 38, 112);
        button(hwnd, "Browser sign-in", LOGIN, 828, 38, 140);
        control(hwnd, "STATIC", "Alias", 0, 12, 80, 150, 20, 0);
        control(hwnd, "STATIC", "PlaceId", 0, 228, 80, 140, 20, 0);
        control(hwnd, "STATIC", "JobId (optional)", 0, 408, 80, 180, 20, 0);
        control(
            hwnd,
            "STATIC",
            "Private-server link (optional)",
            0,
            12,
            142,
            390,
            20,
            0,
        );
        state.alias = control(
            hwnd,
            "EDIT",
            "",
            202,
            12,
            104,
            200,
            26,
            WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
        );
        state.place = control(
            hwnd,
            "EDIT",
            "",
            203,
            228,
            104,
            164,
            26,
            WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
        );
        state.job = control(
            hwnd,
            "EDIT",
            "",
            204,
            408,
            104,
            350,
            26,
            WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
        );
        state.rejoin = control(
            hwnd,
            "BUTTON",
            "Automatic rejoin",
            205,
            780,
            104,
            190,
            26,
            BS_AUTO3STATE as u32 | WS_TABSTOP,
        );
        state.private = control(
            hwnd,
            "EDIT",
            "",
            206,
            12,
            166,
            500,
            26,
            WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
        );
        SendMessageW(state.private, EM_SETLIMITTEXT, 2048, 0);
        button(hwnd, "Start", START, 526, 166, 70);
        button(hwnd, "Stop", STOP, 604, 166, 70);
        button(hwnd, "Restart", RESTART, 682, 166, 80);
        button(hwnd, "Remove", REMOVE, 770, 166, 80);
        state.list = control(
            hwnd,
            "SysListView32",
            "",
            207,
            12,
            216,
            1080,
            400,
            WS_BORDER | WS_TABSTOP | LVS_REPORT | LVS_SHOWSELALWAYS,
        );
        SendMessageW(
            state.list,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            0,
            (LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER) as isize,
        );
        for (i, (label, width)) in [
            ("Account", 220),
            ("PID", 70),
            ("Status", 135),
            ("Target", 140),
            ("Failures", 65),
            ("Rejoin", 65),
            ("Last error", 410),
        ]
        .into_iter()
        .enumerate()
        {
            let mut name = wide(label);
            let mut column: LVCOLUMNW = std::mem::zeroed();
            column.mask = LVCF_TEXT | LVCF_WIDTH;
            column.pszText = name.as_mut_ptr();
            column.cx = width;
            SendMessageW(
                state.list,
                LVM_INSERTCOLUMNW,
                i,
                &column as *const _ as isize,
            );
        }
        state.status = control(hwnd, "STATIC", "", 208, 12, 638, 1080, 28, 0);
        // Group existing direct children into the Accounts page, then offset below tabs.
        struct Children {
            parent: HWND,
            controls: Vec<HWND>,
        }
        unsafe extern "system" fn collect(child: HWND, data: LPARAM) -> i32 {
            let data = unsafe { &mut *(data as *mut Children) };
            if unsafe { GetParent(child) } == data.parent {
                data.controls.push(child);
            }
            1
        }
        let mut children = Children {
            parent: hwnd,
            controls: Vec::new(),
        };
        EnumChildWindows(hwnd, Some(collect), &mut children as *mut _ as isize);
        state.pages[0] = children
            .controls
            .into_iter()
            .filter(|child| *child != state.rejoin && *child != state.status)
            .collect();
        for child in &state.pages[0] {
            let mut rect: RECT = std::mem::zeroed();
            GetWindowRect(*child, &mut rect);
            let mut point = POINT {
                x: rect.left,
                y: rect.top,
            };
            ScreenToClient(hwnd, &mut point);
            MoveWindow(
                *child,
                point.x,
                point.y + 40,
                rect.right - rect.left,
                rect.bottom - rect.top,
                1,
            );
        }
        state.tabs = control(
            hwnd,
            "SysTabControl32",
            "",
            TABS,
            12,
            8,
            950,
            30,
            WS_TABSTOP,
        );
        for (index, title) in ["Accounts", "Settings", "Changelog"].iter().enumerate() {
            let mut title = wide(title);
            let mut item: TCITEMW = std::mem::zeroed();
            item.mask = TCIF_TEXT;
            item.pszText = title.as_mut_ptr();
            SendMessageW(
                state.tabs,
                TCM_INSERTITEMW,
                index,
                &item as *const _ as isize,
            );
        }
        state.settings_summary = control(
            hwnd,
            "STATIC",
            "Select accounts in the Accounts tab to change their settings.",
            212,
            12,
            56,
            1000,
            60,
            0,
        );
        MoveWindow(state.rejoin, 12, 110, 280, 28, 1);
        button(hwnd, "API token", TOKEN, 12, 148, 120);
        let settings_note = control(
            hwnd,
            "STATIC",
            "Automatic rejoin waits 30 seconds for reconnection, then retries with backoff. Turn it off to keep the client open without automatic replacement. Changes are saved immediately.",
            213,
            12,
            184,
            950,
            42,
            0,
        );
        state.pages[1] = vec![
            state.settings_summary,
            state.rejoin,
            GetDlgItem(hwnd, TOKEN as i32),
            settings_note,
        ];
        let discord_note = control(
            hwnd,
            "STATIC",
            "Discord alerts - create a webhook in your Discord channel's Integrations settings.",
            0,
            12,
            238,
            1030,
            22,
            0,
        );
        state.discord_enabled = control(
            hwnd,
            "BUTTON",
            "Enable Discord notifications",
            DISCORD_ENABLE,
            12,
            264,
            300,
            24,
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
        );
        state.discord_recovery = control(
            hwnd,
            "BUTTON",
            "Also notify for rejoin and back online",
            DISCORD_RECOVERY,
            330,
            264,
            400,
            24,
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
        );
        let webhook_note = control(
            hwnd,
            "STATIC",
            "Webhook URL (hidden). Blank keeps the saved webhook. Applies to all accounts.",
            0,
            12,
            294,
            1040,
            22,
            0,
        );
        state.discord_url = control(
            hwnd,
            "EDIT",
            "",
            DISCORD_URL,
            12,
            320,
            1040,
            26,
            WS_BORDER | WS_TABSTOP | ES_PASSWORD as u32 | ES_AUTOHSCROLL as u32,
        );
        SendMessageW(state.discord_url, EM_SETLIMITTEXT, 512, 0);
        button(hwnd, "Save webhook", DISCORD_SAVE, 12, 354, 150);
        button(hwnd, "Send test", DISCORD_TEST, 174, 354, 120);
        button(hwnd, "Remove webhook", DISCORD_REMOVE, 306, 354, 160);
        state.discord_status = control(hwnd, "STATIC", "", 221, 12, 390, 1040, 38, 0);
        let activity_note = control(
            hwnd,
            "STATIC",
            "Recent activity (latest first; this session, including when Discord is off)",
            0,
            12,
            432,
            1040,
            22,
            0,
        );
        state.activity = control(
            hwnd,
            "EDIT",
            "",
            222,
            12,
            458,
            1040,
            160,
            WS_BORDER
                | WS_VSCROLL
                | WS_TABSTOP
                | ES_MULTILINE as u32
                | ES_READONLY as u32
                | ES_AUTOVSCROLL as u32,
        );
        let discord_controls = [
            discord_note,
            state.discord_enabled,
            state.discord_recovery,
            webhook_note,
            state.discord_url,
            GetDlgItem(hwnd, DISCORD_SAVE as i32),
            GetDlgItem(hwnd, DISCORD_TEST as i32),
            GetDlgItem(hwnd, DISCORD_REMOVE as i32),
            state.discord_status,
            activity_note,
            state.activity,
        ];
        state.pages[1].extend(discord_controls);
        state.changelog = control(
            hwnd,
            "EDIT",
            CHANGELOG,
            214,
            12,
            52,
            1080,
            560,
            WS_BORDER
                | WS_VSCROLL
                | WS_TABSTOP
                | ES_MULTILINE as u32
                | ES_READONLY as u32
                | ES_AUTOVSCROLL as u32,
        );
        state.pages[2] = vec![state.changelog];
        button(hwnd, "Exit", EXIT, 976, 8, 80);
        state.switch_page(0);
        state.select_fields();
        let mut tray: NOTIFYICONDATAW = std::mem::zeroed();
        tray.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        tray.hWnd = hwnd;
        tray.uID = 1;
        tray.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        tray.uCallbackMessage = TRAY_MSG;
        tray.hIcon = wc.hIcon;
        let tip = wide("RoLauncher — double-click to open");
        tray.szTip[..tip.len()].copy_from_slice(&tip);
        Shell_NotifyIconW(NIM_ADD, &tray);
        SetTimer(hwnd, 1, 1000, None);
        state.refresh();
        let mut bounds: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut bounds);
        MoveWindow(
            state.list,
            12,
            256,
            bounds.right - 24,
            (bounds.bottom - 306).max(80),
            1,
        );
        MoveWindow(
            state.status,
            12,
            bounds.bottom - 38,
            bounds.right - 24,
            28,
            1,
        );
        #[cfg(test)]
        if smoke {
            state.smoke_actions(hwnd);
        }
        if [
            state.list,
            state.cookie,
            state.alias,
            state.place,
            state.job,
            state.private,
            state.rejoin,
            state.status,
            state.tabs,
            state.settings_summary,
            state.changelog,
            state.discord_url,
            state.discord_enabled,
            state.discord_recovery,
            state.discord_status,
            state.activity,
        ]
        .iter()
        .any(|h| h.is_null())
        {
            DestroyWindow(hwnd);
            return Err("Native control creation failed".into());
        }
        drop(state);
        if smoke {
            SetTimer(hwnd, 2, 1000, None);
        } else {
            ShowWindow(hwnd, SW_SHOWNORMAL);
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            if msg.message == WM_KEYDOWN
                && msg.hwnd == GetDlgItem(hwnd, 207)
                && msg.wParam == 0x41
                && GetKeyState(VK_CONTROL as i32) < 0
            {
                let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut RefCell<Ui>;
                let mut ui = (&*state).borrow_mut();
                ui.select_all();
                continue;
            }
            if IsDialogMessageW(hwnd, &msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_window_creates_controls_and_exits_hidden() {
        let path = std::env::temp_dir().join(format!("rbx-ui-test-{}", uuid::Uuid::new_v4()));
        let store = crate::store::Store::new(path.clone()).unwrap();
        let mut database = crate::model::Database {
            version: 1,
            ..Default::default()
        };
        for index in 0..3 {
            let mut account = Account::new(index.to_string(), format!("Test {index}"));
            account.target = Some(Target {
                place_id: 1,
                job_id: None,
                private_server_link: None,
            });
            database.accounts.push(crate::model::SavedAccount {
                account,
                encrypted_session: crate::platform::protect("test-session").unwrap(),
            });
        }
        store.save(&database).unwrap();
        let engine = Engine::open(store, "UI smoke test".into()).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        assert!(run_window(engine.clone(), runtime.handle().clone(), 38471, true).is_ok());
        drop(engine);
        drop(runtime);
        std::fs::remove_dir_all(path).unwrap();
    }
}
