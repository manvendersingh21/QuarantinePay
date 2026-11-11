//! Browser behavior is written in Rust; JavaScript only loads generated WASM.
#[cfg(target_arch = "wasm32")]
mod browser {
    use wasm_bindgen::{JsCast, prelude::*};
    #[wasm_bindgen(start)]
    pub fn start() -> Result<(), JsValue> {
        let window = web_sys::window().ok_or(JsValue::from_str("window missing"))?;
        let document = window
            .document()
            .ok_or(JsValue::from_str("document missing"))?;
        if document
            .query_selector("body")?
            .and_then(|e| e.get_attribute("data-auth"))
            .as_deref()
            != Some("true")
        {
            return Ok(());
        }
        let event_id = document
            .query_selector("body")?
            .and_then(|e| e.get_attribute("data-event-id"))
            .unwrap_or_else(|| "0".into());
        let tick = Closure::<dyn Fn()>::new(move || {
            if let Ok(nodes) = document.query_selector_all("[data-deadline]") {
                for i in 0..nodes.length() {
                    if let Some(el) = nodes
                        .item(i)
                        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
                    {
                        let raw = el.get_attribute("data-deadline").unwrap_or_default();
                        let seconds = ((js_sys::Date::parse(&raw) - js_sys::Date::now()) / 1000.0)
                            .ceil()
                            .max(0.0);
                        if seconds.is_finite() {
                            let total = seconds as u64;
                            let text = if total >= 3600 {
                                format!("{}h {:02}m remaining", total / 3600, (total % 3600) / 60)
                            } else if total >= 60 {
                                format!("{}m {:02}s remaining", total / 60, total % 60)
                            } else {
                                format!("{total}s remaining")
                            };
                            el.set_text_content(Some(&text));
                        }
                    }
                }
            }
        });
        window.set_interval_with_callback_and_timeout_and_arguments_0(
            tick.as_ref().unchecked_ref(),
            1000,
        )?;
        tick.forget();
        let source = web_sys::EventSource::new(&format!("/api/events?after={event_id}"))?;
        let callback =
            Closure::<dyn Fn(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
                if event.data().as_string().is_some_and(|s| s != "[]") {
                    if let Some(w) = web_sys::window() {
                        let _ = w.location().reload();
                    }
                }
            });
        source.add_event_listener_with_callback("audit", callback.as_ref().unchecked_ref())?;
        callback.forget();
        std::mem::forget(source);
        Ok(())
    }
}
