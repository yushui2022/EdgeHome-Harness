use std::{
    collections::HashMap,
    fmt, fs,
    path::Path,
    time::{Duration, Instant},
};

use edgehome_core::{
    Action, CommandParams, DeviceId, DryRunPlan, ExecutionPlan, ExecutionResult, NormalizedCommand,
    PolicyDecision,
};
use edgehome_registry::{BackendKind, DeviceRecord};
use rumqttc::{Client, Event, MqttOptions, Outgoing, QoS, RecvTimeoutError};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Executor, ExecutorError, ExecutorResult};

const DEFAULT_MQTT_BROKER_URL_ENV: &str = "EDGEHOME_MQTT_BROKER_URL";
const DEFAULT_MQTT_USERNAME_ENV: &str = "EDGEHOME_MQTT_USERNAME";
const DEFAULT_MQTT_PASSWORD_ENV: &str = "EDGEHOME_MQTT_PASSWORD";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MqttConfig {
    pub broker_url: Option<String>,
    pub broker_url_env: String,
    pub username_env: Option<String>,
    pub password_env: Option<String>,
    pub client_id: String,
    pub request_timeout_ms: u64,
    pub execute_enabled: bool,
    pub qos: u8,
    pub retain: bool,
}

impl Default for MqttConfig {
    fn default() -> Self {
        Self {
            broker_url: None,
            broker_url_env: DEFAULT_MQTT_BROKER_URL_ENV.to_owned(),
            username_env: Some(DEFAULT_MQTT_USERNAME_ENV.to_owned()),
            password_env: Some(DEFAULT_MQTT_PASSWORD_ENV.to_owned()),
            client_id: "edgehome-harness".to_owned(),
            request_timeout_ms: 5_000,
            execute_enabled: false,
            qos: 0,
            retain: false,
        }
    }
}

impl MqttConfig {
    pub fn load_from_path(path: impl AsRef<Path>) -> ExecutorResult<Self> {
        let path = path.as_ref();
        let content = fs::read_to_string(path).map_err(|source| ExecutorError::MqttConfigRead {
            path: path.to_path_buf(),
            source,
        })?;
        let config =
            serde_yaml::from_str(&content).map_err(|source| ExecutorError::MqttConfigParse {
                path: path.to_path_buf(),
                source,
            })?;
        Ok(config)
    }
}

#[derive(Clone)]
pub struct MqttSecrets {
    broker_url: String,
    username: Option<String>,
    password: Option<String>,
}

impl MqttSecrets {
    pub fn new(
        broker_url: impl Into<String>,
        username: Option<String>,
        password: Option<String>,
    ) -> ExecutorResult<Self> {
        let broker_url = broker_url.into().trim().to_owned();
        if broker_url.is_empty() {
            return Err(ExecutorError::MqttSecretMissing {
                env_var: DEFAULT_MQTT_BROKER_URL_ENV.to_owned(),
            });
        }
        parse_broker_url(&broker_url)?;
        Ok(Self {
            broker_url,
            username: empty_to_none(username),
            password: empty_to_none(password),
        })
    }

    pub fn load(config: &MqttConfig) -> ExecutorResult<Option<Self>> {
        let broker_url = config.broker_url.as_ref().and_then(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        });
        let broker_url = broker_url.or_else(|| env_value(&config.broker_url_env));
        let Some(broker_url) = broker_url else {
            return Ok(None);
        };

        let username = config.username_env.as_deref().and_then(env_value);
        let password = config.password_env.as_deref().and_then(env_value);
        Self::new(broker_url, username, password).map(Some)
    }

    pub fn load_required(config: &MqttConfig) -> ExecutorResult<Self> {
        Self::load(config)?.ok_or_else(|| ExecutorError::MqttSecretMissing {
            env_var: config.broker_url_env.clone(),
        })
    }

    fn broker_url(&self) -> &str {
        &self.broker_url
    }

    fn username(&self) -> Option<&str> {
        self.username.as_deref()
    }

    fn password(&self) -> Option<&str> {
        self.password.as_deref()
    }
}

impl fmt::Debug for MqttSecrets {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MqttSecrets")
            .field("broker_url", &"<configured>")
            .field("username", &self.username.as_ref().map(|_| "<redacted>"))
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MqttPublishRequest {
    pub topic: String,
    pub qos: u8,
    pub retain: bool,
    pub payload: Value,
}

pub trait MqttPublisher {
    fn publish(
        &self,
        config: &MqttConfig,
        secrets: &MqttSecrets,
        request: &MqttPublishRequest,
    ) -> ExecutorResult<()>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct RumqttcMqttPublisher;

impl MqttPublisher for RumqttcMqttPublisher {
    fn publish(
        &self,
        config: &MqttConfig,
        secrets: &MqttSecrets,
        request: &MqttPublishRequest,
    ) -> ExecutorResult<()> {
        let endpoint = parse_broker_url(secrets.broker_url())?;
        let mut options = MqttOptions::new(&config.client_id, endpoint.host, endpoint.port);
        options.set_keep_alive(Duration::from_secs(5));
        if let Some(username) = secrets.username() {
            options.set_credentials(username, secrets.password().unwrap_or_default());
        }

        let (client, mut connection) = Client::new(options, 10);
        let payload = serde_json::to_vec(&request.payload)?;
        client
            .publish(
                request.topic.clone(),
                qos_from_u8(request.qos)?,
                request.retain,
                payload,
            )
            .map_err(|error| ExecutorError::MqttPublishFailed(error.to_string()))?;

        wait_for_publish(&mut connection, config.request_timeout_ms)
    }
}

#[derive(Debug, Clone)]
pub struct MqttExecutor<P = RumqttcMqttPublisher> {
    config: MqttConfig,
    secrets: Option<MqttSecrets>,
    routes: HashMap<DeviceId, String>,
    publisher: P,
}

impl MqttExecutor<RumqttcMqttPublisher> {
    pub fn new(config: MqttConfig, secrets: Option<MqttSecrets>) -> Self {
        Self::with_publisher(config, secrets, RumqttcMqttPublisher)
    }

    pub fn from_config(config: &MqttConfig, secrets: Option<MqttSecrets>) -> Self {
        Self::new(config.clone(), secrets)
    }
}

impl<P> MqttExecutor<P> {
    pub fn with_publisher(config: MqttConfig, secrets: Option<MqttSecrets>, publisher: P) -> Self {
        Self {
            config,
            secrets,
            routes: HashMap::new(),
            publisher,
        }
    }

    pub fn from_device_records(
        config: MqttConfig,
        secrets: Option<MqttSecrets>,
        devices: &[DeviceRecord],
        publisher: P,
    ) -> ExecutorResult<Self> {
        let mut executor = Self::with_publisher(config, secrets, publisher);
        for device in devices
            .iter()
            .filter(|device| device.backend == BackendKind::Mqtt)
        {
            executor = executor.with_route(device.device_id.clone(), &device.backend_entity_id)?;
        }
        Ok(executor)
    }

    pub fn with_route(
        mut self,
        device_id: DeviceId,
        topic: impl Into<String>,
    ) -> ExecutorResult<Self> {
        let topic = topic.into();
        validate_mqtt_topic(&topic)?;
        self.routes.insert(device_id, topic);
        Ok(self)
    }

    pub fn translate_plan(&self, plan: &ExecutionPlan) -> ExecutorResult<MqttPublishRequest> {
        let topic = self
            .routes
            .get(&plan.target)
            .ok_or_else(|| ExecutorError::MissingMqttRoute(plan.target.0.clone()))?
            .clone();
        validate_mqtt_topic(&topic)?;
        Ok(MqttPublishRequest {
            topic,
            qos: self.config.qos,
            retain: self.config.retain,
            payload: mqtt_action_payload_from_parts(&plan.action, &plan.params),
        })
    }
}

impl<P> Executor for MqttExecutor<P>
where
    P: MqttPublisher + fmt::Debug,
{
    fn dry_run(&self, plan: &DryRunPlan) -> ExecutorResult<DryRunPlan> {
        if plan.backend != "mqtt" {
            return Err(ExecutorError::ExecutorBackendMismatch {
                expected: "mqtt".to_owned(),
                actual: plan.backend.clone(),
            });
        }
        Ok(plan.clone())
    }

    fn execute(&self, plan: &ExecutionPlan) -> ExecutorResult<ExecutionResult> {
        if !self.config.execute_enabled {
            return Err(ExecutorError::ExecuteDisabled);
        }
        if plan.policy == PolicyDecision::Deny {
            return Err(ExecutorError::PolicyDenied);
        }

        let request = self.translate_plan(plan)?;
        let secrets = self
            .secrets
            .as_ref()
            .ok_or_else(|| ExecutorError::MqttSecretMissing {
                env_var: self.config.broker_url_env.clone(),
            })?;
        self.publisher.publish(&self.config, secrets, &request)?;
        Ok(ExecutionResult {
            success: true,
            message: format!("mqtt publish accepted for {}", request.topic),
            raw_backend_response: Some(json!({
                "backend": "mqtt",
                "topic": request.topic,
                "qos": request.qos,
                "retain": request.retain,
                "payload": request.payload,
            })),
        })
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MqttAdapter;

impl crate::BackendAdapter for MqttAdapter {
    fn kind(&self) -> BackendKind {
        BackendKind::Mqtt
    }

    fn dry_run_payload(
        &self,
        device: &DeviceRecord,
        command: &NormalizedCommand,
        _plan: &ExecutionPlan,
    ) -> ExecutorResult<Value> {
        mqtt_payload(device, command)
    }
}

pub fn mqtt_payload(device: &DeviceRecord, command: &NormalizedCommand) -> ExecutorResult<Value> {
    let topic = device.backend_entity_id.trim();
    if topic.is_empty() {
        return Err(ExecutorError::MissingMqttRoute(device.device_id.0.clone()));
    }
    validate_mqtt_topic(topic)?;

    Ok(json!({
        "backend": "mqtt",
        "device_id": device.device_id,
        "topic": topic,
        "qos": 0,
        "retain": false,
        "room": device.room,
        "device_type": device.device_type,
        "action": command.action,
        "payload": mqtt_action_payload(command),
        "condition": {
            "time_after": command.params.time_after,
        }
    }))
}

pub fn mqtt_action_payload(command: &NormalizedCommand) -> Value {
    mqtt_action_payload_from_parts(&command.action, &command.params)
}

pub fn validate_mqtt_topic(topic: &str) -> ExecutorResult<()> {
    if topic.is_empty()
        || topic != topic.trim()
        || topic.starts_with('$')
        || topic.contains('#')
        || topic.contains('+')
        || topic.chars().any(|character| character.is_control())
    {
        return Err(ExecutorError::InvalidMqttTopic(topic.to_owned()));
    }
    Ok(())
}

fn mqtt_action_payload_from_parts(action: &Action, params: &CommandParams) -> Value {
    match action {
        Action::TurnOn => json!({ "power": "on" }),
        Action::TurnOff => json!({ "power": "off" }),
        Action::SetBrightness => json!({ "brightness_pct": params.brightness }),
        Action::IncreaseBrightness => json!({ "brightness_delta": "increase" }),
        Action::DecreaseBrightness => json!({ "brightness_delta": "decrease" }),
        Action::SetTemperature => json!({ "temperature": params.temperature }),
        Action::SetMode => json!({ "mode": params.mode }),
        Action::Open => json!({ "state": "open" }),
        Action::Close => json!({ "state": "close" }),
        Action::Lock => json!({ "state": "locked" }),
        Action::Unlock => json!({ "state": "unlocked" }),
        Action::Unknown => json!({ "operation": "unknown" }),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MqttBrokerEndpoint {
    host: String,
    port: u16,
}

fn parse_broker_url(value: &str) -> ExecutorResult<MqttBrokerEndpoint> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ExecutorError::InvalidMqttBrokerUrl(value.to_owned()));
    }
    let authority = value.strip_prefix("mqtt://").unwrap_or(value);
    if authority.contains('/') || authority.contains('?') || authority.contains('#') {
        return Err(ExecutorError::InvalidMqttBrokerUrl(value.to_owned()));
    }

    let (host, port) = if let Some((host, port)) = authority.rsplit_once(':') {
        let port = port
            .parse::<u16>()
            .map_err(|_| ExecutorError::InvalidMqttBrokerUrl(value.to_owned()))?;
        (host, port)
    } else {
        (authority, 1883)
    };

    if host.trim().is_empty() || host.chars().any(char::is_whitespace) {
        return Err(ExecutorError::InvalidMqttBrokerUrl(value.to_owned()));
    }

    Ok(MqttBrokerEndpoint {
        host: host.to_owned(),
        port,
    })
}

fn qos_from_u8(value: u8) -> ExecutorResult<QoS> {
    match value {
        0 => Ok(QoS::AtMostOnce),
        1 => Ok(QoS::AtLeastOnce),
        2 => Ok(QoS::ExactlyOnce),
        other => Err(ExecutorError::InvalidMqttQos(other)),
    }
}

fn wait_for_publish(
    connection: &mut rumqttc::Connection,
    request_timeout_ms: u64,
) -> ExecutorResult<()> {
    let timeout = Duration::from_millis(request_timeout_ms.max(1));
    let deadline = Instant::now() + timeout;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(ExecutorError::MqttPublishFailed(
                "publish timed out".to_owned(),
            ));
        }
        let remaining = deadline - now;
        let step = remaining.min(Duration::from_millis(250));
        match connection.recv_timeout(step) {
            Ok(Ok(Event::Outgoing(Outgoing::Publish(_)))) => return Ok(()),
            Ok(Ok(_)) => {}
            Ok(Err(error)) => return Err(ExecutorError::MqttPublishFailed(error.to_string())),
            Err(RecvTimeoutError::Timeout) => {
                return Err(ExecutorError::MqttPublishFailed(
                    "publish timed out".to_owned(),
                ));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(ExecutorError::MqttPublishFailed(
                    "connection disconnected".to_owned(),
                ));
            }
        }
    }
}

fn empty_to_none(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    })
}

fn env_value(env_var: &str) -> Option<String> {
    let env_var = env_var.trim();
    if env_var.is_empty() {
        return None;
    }
    std::env::var(env_var).ok().and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    })
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{Arc, Mutex},
        thread,
        time::Duration,
    };

    use super::*;
    use edgehome_core::{CommandParams, DeviceId};

    fn plan() -> ExecutionPlan {
        ExecutionPlan {
            trace_id: Some("tr_test".to_owned()),
            dry_run: true,
            target: DeviceId::new("hallway_light").expect("device id"),
            action: Action::TurnOn,
            params: CommandParams::default(),
            policy: PolicyDecision::Allow,
        }
    }

    #[test]
    fn config_defaults_to_execution_disabled() {
        let config = MqttConfig::default();

        assert!(!config.execute_enabled);
        assert_eq!(config.broker_url_env, "EDGEHOME_MQTT_BROKER_URL");
        assert_eq!(config.qos, 0);
        assert!(!config.retain);
    }

    #[test]
    fn secret_debug_redacts_credentials() {
        let secrets = MqttSecrets::new(
            "mqtt://127.0.0.1:1883",
            Some("user".to_owned()),
            Some("super-secret".to_owned()),
        )
        .expect("secrets");

        let debug = format!("{secrets:?}");

        assert!(debug.contains("redacted"));
        assert!(!debug.contains("super-secret"));
        assert!(!debug.contains("127.0.0.1"));
    }

    #[test]
    fn broker_url_parser_accepts_default_port() {
        assert_eq!(
            parse_broker_url("mqtt://localhost").expect("endpoint"),
            MqttBrokerEndpoint {
                host: "localhost".to_owned(),
                port: 1883,
            }
        );
    }

    #[test]
    fn broker_url_parser_rejects_paths() {
        let error =
            parse_broker_url("mqtt://localhost:1883/private").expect_err("path is not accepted");

        assert!(matches!(error, ExecutorError::InvalidMqttBrokerUrl(_)));
    }

    #[test]
    fn executor_execute_is_disabled_by_default() {
        let executor = MqttExecutor::new(MqttConfig::default(), None)
            .with_route(
                DeviceId::new("hallway_light").expect("device id"),
                "home/hallway/light/set",
            )
            .expect("route");

        let error = executor.execute(&plan()).expect_err("disabled");

        assert!(matches!(error, ExecutorError::ExecuteDisabled));
    }

    #[test]
    fn executor_translates_plan_to_publish_request() {
        let executor = MqttExecutor::new(MqttConfig::default(), None)
            .with_route(
                DeviceId::new("hallway_light").expect("device id"),
                "home/hallway/light/set",
            )
            .expect("route");

        let request = executor.translate_plan(&plan()).expect("request");

        assert_eq!(
            request,
            MqttPublishRequest {
                topic: "home/hallway/light/set".to_owned(),
                qos: 0,
                retain: false,
                payload: json!({ "power": "on" }),
            }
        );
    }

    #[derive(Debug, Default, Clone)]
    struct RecordingPublisher {
        requests: Arc<Mutex<Vec<MqttPublishRequest>>>,
    }

    impl MqttPublisher for RecordingPublisher {
        fn publish(
            &self,
            _config: &MqttConfig,
            _secrets: &MqttSecrets,
            request: &MqttPublishRequest,
        ) -> ExecutorResult<()> {
            self.requests.lock().expect("lock").push(request.clone());
            Ok(())
        }
    }

    #[test]
    fn executor_publishes_with_injected_publisher_when_enabled() {
        let publisher = RecordingPublisher::default();
        let requests = publisher.requests.clone();
        let config = MqttConfig {
            execute_enabled: true,
            broker_url: Some("mqtt://127.0.0.1:1883".to_owned()),
            ..MqttConfig::default()
        };
        let secrets = MqttSecrets::load_required(&config).expect("secrets");
        let executor = MqttExecutor::with_publisher(config, Some(secrets), publisher)
            .with_route(
                DeviceId::new("hallway_light").expect("device id"),
                "home/hallway/light/set",
            )
            .expect("route");

        let result = executor.execute(&plan()).expect("published");

        assert!(result.success);
        assert_eq!(
            requests.lock().expect("lock").as_slice(),
            [MqttPublishRequest {
                topic: "home/hallway/light/set".to_owned(),
                qos: 0,
                retain: false,
                payload: json!({ "power": "on" }),
            }]
        );
        let serialized = serde_json::to_string(&result).expect("serialize");
        assert!(!serialized.contains("EDGEHOME_MQTT_PASSWORD"));
    }

    #[test]
    fn rumqttc_publisher_sends_publish_to_local_broker() {
        let (broker_url, handle) = spawn_mqtt_broker();
        let config = MqttConfig {
            broker_url: Some(broker_url.clone()),
            client_id: "edgehome-harness-test".to_owned(),
            request_timeout_ms: 2_000,
            ..MqttConfig::default()
        };
        let secrets = MqttSecrets::new(broker_url, None, None).expect("secrets");
        let request = MqttPublishRequest {
            topic: "home/hallway/light/set".to_owned(),
            qos: 0,
            retain: false,
            payload: json!({ "power": "on" }),
        };

        RumqttcMqttPublisher
            .publish(&config, &secrets, &request)
            .expect("publish to local broker");
        let captured = handle.join().expect("broker thread");

        assert_eq!(captured.topic, "home/hallway/light/set");
        assert_eq!(captured.payload, json!({ "power": "on" }));
        assert!(!format!("{secrets:?}").contains("127.0.0.1"));
    }

    #[derive(Debug, Clone, PartialEq)]
    struct CapturedMqttPublish {
        topic: String,
        payload: Value,
    }

    fn spawn_mqtt_broker() -> (String, thread::JoinHandle<CapturedMqttPublish>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local mqtt broker");
        let address = listener.local_addr().expect("local address");
        let broker_url = format!("mqtt://{address}");
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept mqtt client");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("set read timeout");

            let connect = read_mqtt_packet(&mut stream).expect("read connect");
            assert_eq!(connect.first().map(|byte| byte >> 4), Some(1));
            stream
                .write_all(&[0x20, 0x02, 0x00, 0x00])
                .expect("write connack");

            loop {
                let packet = read_mqtt_packet(&mut stream).expect("read packet");
                if packet.first().map(|byte| byte >> 4) == Some(3) {
                    return parse_publish_packet(&packet);
                }
            }
        });
        (broker_url, handle)
    }

    fn read_mqtt_packet(stream: &mut std::net::TcpStream) -> std::io::Result<Vec<u8>> {
        let mut packet = Vec::new();
        let mut first = [0_u8; 1];
        stream.read_exact(&mut first)?;
        packet.push(first[0]);

        let mut remaining_length = 0_usize;
        let mut multiplier = 1_usize;
        loop {
            let mut encoded = [0_u8; 1];
            stream.read_exact(&mut encoded)?;
            packet.push(encoded[0]);
            remaining_length += usize::from(encoded[0] & 0x7f) * multiplier;
            if encoded[0] & 0x80 == 0 {
                break;
            }
            multiplier *= 128;
        }

        let mut body = vec![0_u8; remaining_length];
        stream.read_exact(&mut body)?;
        packet.extend_from_slice(&body);
        Ok(packet)
    }

    fn parse_publish_packet(packet: &[u8]) -> CapturedMqttPublish {
        let (header_len, remaining_length) = decode_remaining_length(packet);
        let end = header_len + remaining_length;
        let body = &packet[header_len..end];
        let topic_len = u16::from_be_bytes([body[0], body[1]]) as usize;
        let topic_start = 2;
        let topic_end = topic_start + topic_len;
        let topic = String::from_utf8(body[topic_start..topic_end].to_vec()).expect("topic utf-8");
        let qos = (packet[0] & 0b0000_0110) >> 1;
        let payload_start = topic_end + if qos > 0 { 2 } else { 0 };
        let payload: Value = serde_json::from_slice(&body[payload_start..]).expect("payload json");

        CapturedMqttPublish { topic, payload }
    }

    fn decode_remaining_length(packet: &[u8]) -> (usize, usize) {
        let mut remaining_length = 0_usize;
        let mut multiplier = 1_usize;
        let mut index = 1_usize;
        loop {
            let encoded = packet[index];
            remaining_length += usize::from(encoded & 0x7f) * multiplier;
            index += 1;
            if encoded & 0x80 == 0 {
                break;
            }
            multiplier *= 128;
        }
        (index, remaining_length)
    }
}
