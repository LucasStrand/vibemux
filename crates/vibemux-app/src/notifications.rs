use std::collections::VecDeque;
use uuid::Uuid;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct AppNotification {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub title: String,
    pub body: String,
    pub subtitle: Option<String>,
    pub read: bool,
    pub timestamp: std::time::Instant,
}

pub struct NotificationManager {
    notifications: VecDeque<AppNotification>,
    max_notifications: usize,
}

impl NotificationManager {
    pub fn new() -> Self {
        Self {
            notifications: VecDeque::new(),
            max_notifications: 100,
        }
    }

    pub fn add(
        &mut self,
        workspace_id: Uuid,
        title: String,
        body: String,
        subtitle: Option<String>,
    ) -> Uuid {
        let id = Uuid::new_v4();
        let notif = AppNotification {
            id,
            workspace_id,
            title: title.clone(),
            body: body.clone(),
            subtitle,
            read: false,
            timestamp: std::time::Instant::now(),
        };

        self.notifications.push_front(notif);
        if self.notifications.len() > self.max_notifications {
            self.notifications.pop_back();
        }

        send_windows_toast(&title, &body);

        id
    }

    pub fn mark_workspace_read(&mut self, workspace_id: Uuid) {
        for notif in &mut self.notifications {
            if notif.workspace_id == workspace_id {
                notif.read = true;
            }
        }
    }

    pub fn unread_count_for(&self, workspace_id: Uuid) -> usize {
        self.notifications
            .iter()
            .filter(|n| n.workspace_id == workspace_id && !n.read)
            .count()
    }

    pub fn has_unread(&self, workspace_id: Uuid) -> bool {
        self.unread_count_for(workspace_id) > 0
    }

    #[allow(dead_code)]
    pub fn recent(&self, limit: usize) -> Vec<&AppNotification> {
        self.notifications.iter().take(limit).collect()
    }

    #[allow(dead_code)]
    pub fn clear_all(&mut self) {
        self.notifications.clear();
    }
}

fn send_windows_toast(title: &str, body: &str) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Command;
        let ps_script = format!(
            r#"
            [Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null
            [Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom, ContentType = WindowsRuntime] | Out-Null
            $template = @"
            <toast>
                <visual>
                    <binding template="ToastGeneric">
                        <text>{title}</text>
                        <text>{body}</text>
                    </binding>
                </visual>
            </toast>
"@
            $xml = New-Object Windows.Data.Xml.Dom.XmlDocument
            $xml.LoadXml($template)
            $toast = [Windows.UI.Notifications.ToastNotification]::new($xml)
            [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier("VibeMux").Show($toast)
            "#,
            title = title.replace('"', "'"),
            body = body.replace('"', "'"),
        );

        let _ = Command::new("powershell")
            .args(["-NoProfile", "-Command", &ps_script])
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .spawn();
    }
}
