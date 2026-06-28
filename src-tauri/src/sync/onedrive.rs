use serde::Deserialize;

/// Microsoft Graph API 应用 ID（已注册的应用）
pub const DEFAULT_CLIENT_ID: &str = "YOUR_CLIENT_ID_HERE";

const TOKEN_URL: &str = "https://login.microsoft.com/common/oauth2/v2.0/token";
const DEVICE_CODE_URL: &str = "https://login.microsoft.com/common/oauth2/v2.0/devicecode";
const GRAPH_BASE: &str = "https://graph.microsoft.com/v1.0/me/drive/special/approot";

#[derive(Debug, Deserialize)]
pub struct DeviceCodeResponse {
    pub user_code: String,
    pub verification_uri: String,
    pub device_code: String,
    pub expires_in: u64,
}

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<u64>,
}

pub struct OneDriveClient {
    client_id: String,
    refresh_token: String,
    access_token: String,
    token_expires_at: i64,
    http: reqwest::Client,
}

impl OneDriveClient {
    pub fn new(client_id: String, refresh_token: String) -> Self {
        Self {
            client_id,
            refresh_token,
            access_token: String::new(),
            token_expires_at: 0,
            http: reqwest::Client::new(),
        }
    }

    /// 如果 access_token 过期则刷新
    pub async fn ensure_token(&mut self) -> Result<(), String> {
        let now = chrono::Utc::now().timestamp();
        // 提前 60 秒刷新
        if self.access_token.is_empty() || now >= self.token_expires_at - 60 {
            self.refresh_access_token().await?;
        }
        Ok(())
    }

    /// 刷新 access_token
    pub async fn refresh_access_token(&mut self) -> Result<(), String> {
        let params = [
            ("grant_type", "refresh_token"),
            ("client_id", self.client_id.as_str()),
            ("refresh_token", self.refresh_token.as_str()),
        ];

        let resp = self
            .http
            .post(TOKEN_URL)
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("刷新 token 请求失败: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("刷新 token 失败 ({}): {}", status, body));
        }

        let token: TokenResponse = resp
            .json()
            .await
            .map_err(|e| format!("解析 token 响应失败: {}", e))?;

        self.access_token = token.access_token;
        if let Some(rt) = token.refresh_token {
            self.refresh_token = rt;
        }
        let expires_in = token.expires_in.unwrap_or(3600);
        self.token_expires_at = chrono::Utc::now().timestamp() + expires_in as i64;

        Ok(())
    }

    /// 上传文件到 OneDrive 应用目录
    pub async fn upload_file(&mut self, path: &str, content: &str) -> Result<(), String> {
        self.ensure_token().await?;

        let url = format!("{GRAPH_BASE}:/{path}:/content");

        let resp = self
            .http
            .put(&url)
            .bearer_auth(&self.access_token)
            .header("Content-Type", "text/plain")
            .body(content.to_string())
            .send()
            .await
            .map_err(|e| format!("上传文件请求失败: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("上传文件失败 ({}): {}", status, body));
        }

        Ok(())
    }

    /// 从 OneDrive 应用目录下载文件，404 返回 Ok(None)
    pub async fn download_file(&mut self, path: &str) -> Result<Option<String>, String> {
        self.ensure_token().await?;

        let url = format!("{GRAPH_BASE}:/{path}:/content");

        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.access_token)
            .send()
            .await
            .map_err(|e| format!("下载文件请求失败: {}", e))?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("下载文件失败 ({}): {}", status, body));
        }

        let text = resp
            .text()
            .await
            .map_err(|e| format!("读取下载内容失败: {}", e))?;

        Ok(Some(text))
    }
}

/// 发起设备码登录流程
pub async fn start_device_code_flow(client_id: &str) -> Result<DeviceCodeResponse, String> {
    let params = [
        ("client_id", client_id),
        ("scope", "Files.ReadWrite.AppFolder offline_access"),
    ];

    let client = reqwest::Client::new();
    let resp = client
        .post(DEVICE_CODE_URL)
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("设备码请求失败: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("获取设备码失败 ({}): {}", status, body));
    }

    let dc: DeviceCodeResponse = resp
        .json()
        .await
        .map_err(|e| format!("解析设备码响应失败: {}", e))?;

    Ok(dc)
}

/// 轮询设备码以获取 token（用户完成授权后成功）
pub async fn poll_device_code(
    client_id: &str,
    device_code: &str,
) -> Result<TokenResponse, String> {
    let params = [
        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ("client_id", client_id),
        ("device_code", device_code),
    ];

    let client = reqwest::Client::new();
    let resp = client
        .post(TOKEN_URL)
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("轮询设备码请求失败: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("轮询设备码失败 ({}): {}", status, body));
    }

    let token: TokenResponse = resp
        .json()
        .await
        .map_err(|e| format!("解析 token 响应失败: {}", e))?;

    Ok(token)
}
