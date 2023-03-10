use crate::status_code;
use crate::{context::RumContext, method::MethodType, router::Router};
use std::io::Write;
use std::str::FromStr;
use std::{io::Read, net::TcpStream};
use tera::Tera;

pub(crate) struct Handler {
    pub(crate) router: Router,
    static_asset_path: Option<String>,
    pub(crate) template_engine: Option<Tera>,
}

impl Handler {
    pub(crate) fn new() -> Handler {
        return Handler {
            router: Router::new(),
            static_asset_path: None,
            template_engine: None,
        };
    }

    pub(crate) fn set_template_engine(&mut self, templates_path: &str) {
        self.template_engine = match Tera::new(templates_path) {
            Ok(t) => Some(t),
            Err(e) => {
                panic!(
                    "Reading path in {} failed!\n{}\n",
                    templates_path,
                    e.to_string()
                );
            }
        };
    }

    pub(crate) fn add_route(
        &mut self,
        method_type: MethodType,
        route: &str,
        controller: fn(&mut RumContext),
    ) {
        let mut route_segs: Vec<&str> = route.trim_end_matches('/').split('/').collect();
        if route_segs[0] != "" {
            route_segs.insert(0, "");
        }
        let router = match self.router.search_route_mut(&route_segs, 0) {
            Some(router) => router,
            None => self.router.modify(route_segs, 0),
        };
        router.add_controller(method_type, controller);
    }

    pub(crate) fn set_static_assets(&mut self, static_asset_path: &str) {
        self.static_asset_path = Some(static_asset_path.to_string());
    }

    pub(crate) fn set_middleware(&mut self, route: &str, handlers: Vec<fn(&mut RumContext)>) {
        let mut route_segs: Vec<&str> = route.trim_end_matches('/').split('/').collect();
        if route_segs[0] != "" {
            route_segs.insert(0, "");
        }
        let router =  match self.router.search_route_mut(&route_segs, 0) {
            Some(router) => router,
            None => self.router.modify(route_segs, 0),
        };
        router.add_middleware(handlers);
    }

    pub(crate) fn handle_connection(&self, mut stream: TcpStream) {
        let mut buffer = [0; 1024];
        stream.read(&mut buffer).unwrap();
        let requests = String::from_utf8_lossy(&buffer[..]);
        let mut http_method_str = "";
        let mut route = "";
        let mut http_ver = "";
        let mut request_header_parsed = false;
        let mut request_body = String::new();
        let context = &mut RumContext::new(self.template_engine.as_ref());
        // parse the request
        for (index, line) in requests.lines().enumerate() {
            // the border of header and body
            if line.len() == 0 {
                request_header_parsed = true;
                continue;
            }
            if index == 0 {
                let mut iter = line.splitn(3, " ");
                http_method_str = iter.next().unwrap();
                let route_with_query = iter.next().unwrap();
                let mut iter_q = route_with_query.split("?");
                route = iter_q.next().unwrap();
                //parse query params
                match iter_q.next() {
                    Some(q) => {
                        let queries = q.split("&").into_iter();
                        for query in queries {
                            let mut iter_qi = query.splitn(2, "=");
                            let key = iter_qi.next().unwrap_or_else(|| "");
                            let val = iter_qi.next().unwrap_or_else(|| "");
                            if key != "" && val != "" {
                                context.set_query_params(key, val);
                            }
                        }
                    }
                    None => {}
                }
                http_ver = iter.next().unwrap();
            } else if !request_header_parsed {
                let mut iter = line.splitn(2, ": ");
                let key = iter.next().unwrap();
                let value = iter.next().unwrap();
                context.set_request_header(key, value);
            } else {
                request_body = format!("{}{}", request_body, line);
            }
        }

        request_body = request_body.trim().to_string();
        let content_type = match context.get_request_header("Content-Type") {
            Some(value) => value,
            None => "",
        };

        // TODO: implement parse for multipart/form-data
        if content_type.contains(&mime::APPLICATION_WWW_FORM_URLENCODED.to_string()) {
            let form_params = request_body.split("&").into_iter();
            for param in form_params {
                let mut iter_fi = param.splitn(2, "=");
                let key = iter_fi.next().unwrap_or_else(|| "");
                let val = iter_fi.next().unwrap_or_else(|| "");
                if key != "" && val != "" {
                    context.set_form_params(key, val);
                }
            }
        }else if  content_type.contains(&mime::APPLICATION_JSON.to_string()) {
            request_body = request_body.chars().filter(|c| !c.is_whitespace()).collect();
        }
        context.set_request_body(request_body);

        let (response, http_status) = match MethodType::from_str(http_method_str) {
            Ok(http_method_type) => {
                let mut route_segs: Vec<&str> = route.trim_end_matches('/').split('/').collect();
                if route_segs[0] != "" {
                    route_segs.insert(0, "");
                }
                let route_seg_slice = &route_segs[..];
                let last_key = route_segs[route_segs.len() - 1];
                match self
                    .router
                    .get_info_and_controller(http_method_type, route_seg_slice)
                {
                    Some((full_route_info, controller)) => {
                        self.router.exec_middleware(full_route_info, 0, context);
                        if !context.has_response() {
                            for index in 0..full_route_info.len() {
                                if full_route_info[index].starts_with(":") {
                                    let key = full_route_info[index].trim_matches(':');
                                    let val = route_seg_slice[index];
                                    context.set_url_params(key, val);
                                }
                            }
                            controller(context);
                        }
                        context.get_response(http_ver)
                    }
                    None => {
                        let static_path = self.static_asset_path.as_ref();
                        if static_path.is_some() {
                            let file_path = format!("{}/{}", *(static_path.unwrap()), last_key);
                            context.file(status_code::OK, &file_path);
                            context.get_response(http_ver)
                        } else {
                            context.default_404(http_ver)
                        }
                    }
                }
            }
            Err(_) => {
                println!("Unknown Method!");
                context.default_400(http_ver)
            }
        };

        stream.write(response.as_bytes()).unwrap();
        stream.flush().unwrap();

        println!("|{}| {} {}", http_method_str, route, http_status);
    }
}
