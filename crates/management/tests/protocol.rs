use management::{Club, Command, Manager, Request};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command as ProcessCommand, Stdio};

struct Host {
    process: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Host {
    fn start() -> Self {
        let mut process = ProcessCommand::new(env!("CARGO_BIN_EXE_league"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let input = process.stdin.take().unwrap();
        let output = BufReader::new(process.stdout.take().unwrap());
        Self {
            process,
            input,
            output,
        }
    }

    fn raw(&mut self, request: &[u8]) -> Value {
        self.input.write_all(request).unwrap();
        self.input.write_all(b"\n").unwrap();
        self.input.flush().unwrap();
        let mut response = String::new();
        assert!(self.output.read_line(&mut response).unwrap() > 0);
        serde_json::from_str(&response).unwrap()
    }

    fn send(&mut self, request: Value) -> Value {
        self.raw(&serde_json::to_vec(&request).unwrap())
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

fn init() -> Value {
    let clubs: Vec<_> = ["a", "b"]
        .into_iter()
        .map(|id| Club {
            id: id.into(),
            name: format!("Club {id}"),
            balance: 1234567,
        })
        .collect();
    let managers: Vec<_> = ["a", "b"]
        .into_iter()
        .map(|id| Manager {
            id: format!("manager-{id}"),
            club_id: id.into(),
        })
        .collect();
    json!({"op": "init", "clubs": clubs, "managers": managers,
        "players": [], "attributes": [], "fixtures": [], "recovery": null,
        "day": 1, "deadline_ms": 1000})
}

fn ready(actor: &str) -> Value {
    json!({"op": "command", "actor": actor, "now_ms": 500,
        "request": Request { id: "ready".into(), day: 1, command: Command::Ready }})
}

#[test]
fn trusted_checkpoint_roundtrip_preserves_receipts_and_cannot_replace_a_live_game() {
    let mut original = Host::start();
    assert_eq!(original.send(init())["ok"], true);
    let receipt = original.send(ready("manager-a"));
    let saved = original.send(json!({"op":"save"}));
    assert_eq!(saved["ok"], true);
    let mut restored = Host::start();
    assert_eq!(
        restored.send(json!({"op":"load", "checkpoint":{"version":999}}))["ok"],
        false
    );
    assert_eq!(
        restored.send(json!({"op":"load", "checkpoint":saved["data"]}))["ok"],
        true
    );
    assert_eq!(restored.send(ready("manager-a")), receipt);
    assert_eq!(
        restored.send(json!({"op":"public"})),
        original.send(json!({"op":"public"}))
    );
    assert_eq!(
        restored.send(json!({"op":"load", "checkpoint":saved["data"]}))["ok"],
        false
    );
}

#[test]
fn trusted_file_checkpoint_is_exclusive_and_loads_without_a_large_stdin_payload() {
    let path = std::env::temp_dir().join(format!(
        "league-checkpoint-protocol-{}.json",
        std::process::id()
    ));
    let mut original = Host::start();
    assert_eq!(original.send(init())["ok"], true);
    assert_eq!(
        original.send(json!({"op":"save_file", "path":path}))["ok"],
        true
    );
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(
        original.send(json!({"op":"save_file", "path":path}))["ok"],
        false
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let mut restored = Host::start();
    assert_eq!(
        restored.send(json!({"op":"load_file", "path":path}))["ok"],
        true
    );
    assert_eq!(
        restored.send(json!({"op":"public"})),
        original.send(json!({"op":"public"}))
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn malformed_input_and_failed_init_do_not_poison_the_host() {
    let mut host = Host::start();
    assert_eq!(host.raw(b"not json")["ok"], false);
    assert_eq!(host.raw(&[0xff])["ok"], false);
    assert_eq!(
        host.send(json!({"op": "public"}))["error"],
        "Not initialized"
    );
    let mut invalid = init();
    invalid["managers"][0]["club_id"] = json!("missing");
    assert_eq!(host.send(invalid)["ok"], false);
    assert_eq!(host.send(init())["ok"], true);
    assert_eq!(host.send(init())["error"], "Already initialized");
    assert_eq!(
        host.send(json!({"op": "observe", "actor": "intruder"}))["error"],
        "Unauthorized"
    );
    assert_eq!(host.send(ready("intruder"))["error"], "Unauthorized");
    assert_eq!(
        host.send(json!({"op": "public", "unexpected": true}))["ok"],
        false
    );
    assert_eq!(host.send(json!({"op": "public"}))["ok"], true);
}

#[test]
fn readiness_tick_observations_and_public_history_are_scoped() {
    let mut host = Host::start();
    assert_eq!(host.send(init())["ok"], true);
    let observation = host.send(json!({"op": "observe", "actor": "manager-a"}));
    assert_eq!(observation["data"]["manager_view"]["club"]["id"], "a");
    assert_eq!(
        observation["data"]["manager_view"]["club"]["balance"],
        1234567
    );
    assert_eq!(observation["data"]["recovery_view"], Value::Null);
    assert_eq!(observation["data"]["deadline_ms"], 1000);
    let tick = json!({"op": "tick", "day": 1, "now_ms": 600, "next_deadline_ms": 2000});
    assert_eq!(host.send(tick.clone())["ok"], false);
    let receipt = host.send(ready("manager-a"));
    assert_eq!(receipt["data"]["result"]["Ok"], "Ready");
    assert_eq!(host.send(ready("manager-a")), receipt);
    assert_eq!(host.send(tick.clone())["ok"], false);
    assert_eq!(host.send(ready("manager-b"))["ok"], true);
    assert_eq!(host.send(tick.clone())["data"]["day"], 2);
    assert_eq!(host.send(tick)["ok"], false);
    let observation = host.send(json!({"op": "observe", "actor": "manager-b"}));
    assert_eq!(observation["data"]["manager_view"]["club"]["id"], "b");
    assert_eq!(observation["data"]["manager_view"]["ready"], false);
    let public = host.send(json!({"op": "public"}));
    assert_eq!(public["data"]["state"]["day"], 2);
    assert_eq!(public["data"]["standings"].as_array().unwrap().len(), 2);
    for private in [
        "balance",
        "match_plan",
        "recovery_view",
        "manager-a",
        "1234567",
        "offers",
    ] {
        assert!(!public.to_string().contains(private), "leaked {private}");
    }
    let history = host.send(json!({"op": "history", "after": 0, "limit": 100}));
    assert_eq!(history["data"]["results"], json!([]));
    assert_eq!(history["data"]["next"], 0);
    assert_eq!(
        host.send(json!({"op": "history", "after": 1, "limit": 100}))["ok"],
        false
    );
    assert_eq!(
        host.send(json!({"op": "tick", "day": 2, "now_ms": 2000,
        "next_deadline_ms": 3000}))["data"]["day"],
        3
    );
}

#[test]
fn oversized_line_is_drained_before_next_request() {
    let mut host = Host::start();
    let response = host.raw(&vec![b'x'; 8 * 1024 * 1024 + 1]);
    assert_eq!(response["ok"], false);
    assert_eq!(host.send(init())["ok"], true);
}

#[test]
fn schedule_can_be_generated_before_init_and_observation_filters_fixtures() {
    let mut host = Host::start();
    let schedule = host.send(json!({"op": "schedule", "club_ids": ["a", "b"],
        "first_day": 1, "spacing_days": 3, "seed": 42}));
    assert_eq!(schedule["ok"], true);
    assert_eq!(schedule["data"]["fixtures"].as_array().unwrap().len(), 2);
    let mut scenario = init();
    scenario["clubs"].as_array_mut().unwrap().push(json!(Club {
        id: "c".into(),
        name: "Third club".into(),
        balance: 987654,
    }));
    scenario["fixtures"] = json!([
        {"id": "own", "day": 1, "home": "a", "away": "b", "seed": 1},
        {"id": "other", "day": 2, "home": "b", "away": "c", "seed": 2}
    ]);
    assert_eq!(host.send(scenario)["ok"], true);
    let observed = host.send(json!({"op": "observe", "actor": "manager-a"}));
    assert_eq!(observed["data"]["fixtures"].as_array().unwrap().len(), 1);
    assert_eq!(observed["data"]["fixtures"][0]["id"], "own");
}
