use super::*;

impl NativeTransport for CodexAppServer {
    fn request(&mut self, method: &str, params: Value) -> Result<Value, NativeError> {
        self.request(method, params).map_err(|error| match error {
            crate::collectors::CollectorError::Protocol { .. } => {
                NativeError::rejected("provider_rejected")
            }
            _ => NativeError::uncertain("provider_transport_lost"),
        })
    }

    fn respond(&mut self, id: Value, result: Value) -> Result<(), NativeError> {
        self.respond(id, result)
            .map_err(|_| NativeError::uncertain("provider_transport_lost"))
    }

    fn drain(&mut self) -> Result<Vec<Value>, NativeError> {
        self.drain_messages()
            .map_err(|_| NativeError::uncertain("provider_transport_lost"))
    }
}

impl NativeTransport for AcpAppServer {
    fn request(&mut self, method: &str, params: Value) -> Result<Value, NativeError> {
        self.request(method, params).map_err(|error| match error {
            crate::collectors::CollectorError::Protocol { .. } => {
                NativeError::rejected("provider_rejected")
            }
            _ => NativeError::uncertain("provider_transport_lost"),
        })
    }

    fn respond(&mut self, id: Value, result: Value) -> Result<(), NativeError> {
        let decision = result
            .get("decision")
            .and_then(Value::as_str)
            .unwrap_or("decline");
        self.respond_permission(id, decision)
            .map_err(|_| NativeError::uncertain("provider_transport_lost"))
    }

    fn drain(&mut self) -> Result<Vec<Value>, NativeError> {
        self.drain_messages()
            .map_err(|_| NativeError::uncertain("provider_transport_lost"))
    }
}
