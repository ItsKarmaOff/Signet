//! Minimal blocking HTTP abstraction so features stay testable without network.
use std::io::Read;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
    /// Final URL after redirects.
    pub url: String,
}

impl Response {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

pub trait Http: Send + Sync {
    /// GET with headers; HTTP error statuses are returned as responses, only transport failures are errors.
    fn get(&self, url: &str, headers: &[(&str, &str)], timeout: Duration, max_bytes: usize) -> Result<Response, String>;

    /// POST of a form-encoded body.
    fn post_form(&self, _url: &str, _body: &str, _timeout: Duration, _max_bytes: usize) -> Result<Response, String> {
        Err("POST not supported".into())
    }
}

/// Real implementation on `ureq`.
pub struct UreqHttp;

impl Http for UreqHttp {
    fn post_form(&self, url: &str, body: &str, timeout: Duration, max_bytes: usize) -> Result<Response, String> {
        let agent = ureq::AgentBuilder::new().timeout(timeout).build();
        let resp = match agent.post(url).set("content-type", "application/x-www-form-urlencoded;charset=UTF-8").send_string(body) {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(e) => return Err(e.to_string()),
        };
        Self::read(resp, max_bytes)
    }

    fn get(&self, url: &str, headers: &[(&str, &str)], timeout: Duration, max_bytes: usize) -> Result<Response, String> {
        let agent = ureq::AgentBuilder::new().timeout(timeout).build();
        let mut req = agent.get(url);
        for (k, v) in headers {
            req = req.set(k, v);
        }
        let resp = match req.call() {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(e) => return Err(e.to_string()),
        };
        let status = resp.status();
        let final_url = resp.get_url().to_string();
        let mut body = Vec::new();
        resp.into_reader().take(max_bytes as u64).read_to_end(&mut body).map_err(|e| e.to_string())?;
        Ok(Response { status, body, url: final_url })
    }
}

impl UreqHttp {
    fn read(resp: ureq::Response, max_bytes: usize) -> Result<Response, String> {
        let status = resp.status();
        let final_url = resp.get_url().to_string();
        let mut body = Vec::new();
        resp.into_reader().take(max_bytes as u64).read_to_end(&mut body).map_err(|e| e.to_string())?;
        Ok(Response { status, body, url: final_url })
    }
}

/// Test double: answers from a closure.
#[cfg(test)]
type FakeHandler = Box<dyn Fn(&str) -> Result<Response, String> + Send + Sync>;

#[cfg(test)]
pub struct FakeHttp(pub FakeHandler);

#[cfg(test)]
impl Http for FakeHttp {
    fn post_form(&self, url: &str, body: &str, _: Duration, _: usize) -> Result<Response, String> {
        (self.0)(&format!("POST {url} {body}"))
    }

    fn get(&self, url: &str, _: &[(&str, &str)], _: Duration, _: usize) -> Result<Response, String> {
        (self.0)(url)
    }
}

#[cfg(test)]
pub fn resp(status: u16, body: &str) -> Result<Response, String> {
    Ok(Response { status, body: body.as_bytes().to_vec(), url: String::new() })
}
