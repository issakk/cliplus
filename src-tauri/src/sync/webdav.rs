pub struct WebDAVClient {
    url: String,
    username: String,
    password: String,
    http: reqwest::Client,
}

impl WebDAVClient {
    pub fn new(url: String, username: String, password: String) -> Self {
        // 确保 URL 末尾没有多余的斜杠
        let url = url.trim_end_matches('/').to_string();
        Self {
            url,
            username,
            password,
            http: reqwest::Client::new(),
        }
    }

    /// 上传文件到 WebDAV 服务
    pub async fn upload_file(&self, path: &str, content: &str) -> Result<(), String> {
        let path = path.trim_start_matches('/');
        let url = format!("{}/{}", self.url, path);

        let resp = self
            .http
            .put(&url)
            .basic_auth(&self.username, Some(&self.password))
            .header("Content-Type", "text/plain")
            .body(content.to_string())
            .send()
            .await
            .map_err(|e| format!("WebDAV 上传请求失败: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("WebDAV 上传失败 ({}): {}", status, body));
        }

        Ok(())
    }

    /// 从 WebDAV 服务下载文件，404 返回 Ok(None)
    pub async fn download_file(&self, path: &str) -> Result<Option<String>, String> {
        let path = path.trim_start_matches('/');
        let url = format!("{}/{}", self.url, path);

        let resp = self
            .http
            .get(&url)
            .basic_auth(&self.username, Some(&self.password))
            .send()
            .await
            .map_err(|e| format!("WebDAV 下载请求失败: {}", e))?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("WebDAV 下载失败 ({}): {}", status, body));
        }

        let text = resp
            .text()
            .await
            .map_err(|e| format!("读取 WebDAV 下载内容失败: {}", e))?;

        Ok(Some(text))
    }
}
