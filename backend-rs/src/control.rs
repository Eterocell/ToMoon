use std::fmt::Display;
use std::process::{Command, Child};
use std::sync::{RwLock, Arc};

use std::{thread, option, fs};
use std::time::{Duration};

use serde_yaml::{Mapping, Value};

use super::settings::{Settings, State};

pub struct ControlRuntime {
    settings: Arc<RwLock<Settings>>,
    state: Arc<RwLock<State>>,
    clash_state: Arc<RwLock<Clash>>
}

impl ControlRuntime {
    pub fn new() -> Self {
        let new_state = State::new();
        let settings_p = settings_path(&new_state.home);
        //TODO: Clash 路径
        let clash = Clash::default();
        Self {
            settings: Arc::new(RwLock::new(super::settings::Settings::open(settings_p).unwrap_or_default().into())),
            state: Arc::new(RwLock::new(new_state)),
            clash_state: Arc::new(RwLock::new(clash)),
        }
    }
    

    pub(crate) fn settings_clone(&self) -> Arc<RwLock<Settings>> {
        self.settings.clone()
    }

    pub(crate) fn state_clone(&self) -> Arc<RwLock<State>> {
        self.state.clone()
    }

    pub fn clash_state_clone(&self) -> Arc<RwLock<Clash>> {
        self.clash_state.clone()
    }

    pub fn run(&self) -> thread::JoinHandle<()>{
        let runtime_settings = self.settings_clone();
        let runtime_state = self.state_clone();

        //save config
        thread::spawn(move || {
            let sleep_duration = Duration::from_millis(1000);
            loop {
                //let start_time = Instant::now();
                { // save to file
                    let state = match runtime_state.read() {
                        Ok(x) => x,
                        Err(e) => {
                            log::error!("runtime failed to acquire state read lock: {}", e);
                            continue;
                        }
                    };
                    if state.dirty {
                        // save settings to file
                        let settings = match runtime_settings.read() {
                            Ok(x) => x,
                            Err(e) => {
                                log::error!("runtime failed to acquire settings read lock: {}", e);
                                continue;
                            }
                        };
                        let settings_json: Settings = settings.clone().into();
                        if let Err(e) = settings_json.save(settings_path(&state.home)) {
                            log::error!("SettingsJson.save({}) error: {}", settings_path(&state.home).display(), e);
                        }
                        //Self::on_set_enable(&settings, &state);
                        drop(state);
                        let mut state = match runtime_state.write() {
                            Ok(x) => x,
                            Err(e) => {
                                log::error!("runtime failed to acquire state write lock: {}", e);
                                continue;
                            }
                        };
                        state.dirty = false;
                    }
                }
                thread::sleep(sleep_duration);
            }
        })
    }
}

fn settings_path<P: AsRef<std::path::Path>>(home: P) -> std::path::PathBuf {
    home.as_ref().join(".config/clashdeck/clashdeck.json")
}

fn get_current_working_dir() -> std::io::Result<std::path::PathBuf> {
    std::env::current_dir()
}

pub struct Clash {
    path : std::path::PathBuf,
    config : std::path::PathBuf,
    instence: Option<Child>
}

#[derive(Debug)]
pub enum ClashErrorKind {
    CoreNotFound,
    ConfigFormatError,
    ConfigNotFound,
    Default
}


#[derive(Debug)]
pub struct ClashError {
    Message : String,
    ErrorKind : ClashErrorKind
}

impl Display for ClashError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Error Kind: {:?}, Error Message: {})", self.ErrorKind, self.Message)
    }
}

impl ClashError {
    fn new() -> Self {
        Self { Message: "".to_string(), ErrorKind: ClashErrorKind::Default }
    }
}

impl Default for Clash {
    fn default() -> Self {
        Self { path: get_current_working_dir().unwrap().join("bin/core/clash"), config: get_current_working_dir().unwrap().join("bin/core/config.yaml"), instence: None}
    }
}

impl Clash {
    pub fn run(&mut self) -> Result<(), ClashError> {
        let clash = Command::new(self.path.clone())
        .arg("-f")
        .arg(self.config.clone())
        .spawn();
        let clash: Result<Child, ClashError> = match clash {
            Ok(x) => Ok(x),
            Err(e) => {
                log::error!("run Clash failed: {}", e);
                //TODO: 开启 Clash 的错误处理
                return Err(ClashError::new());
            }
        };
        self.instence = Some(clash.unwrap());
        Ok(())
    }

    pub fn stop(&mut self) {
        let instance = self.instence.as_mut();
        match instance {
            Some(mut x) => {
                //TODO: 错误处理
                x.kill().unwrap();
                x.wait().unwrap();
            },
            None => {
                //Not launch Clash yet...
            }
        };
    }

    fn change_config(&self) {
        let path = self.config.clone();
        let config = fs::read_to_string(path).unwrap();
        let mut yaml: serde_yaml::Value = serde_yaml::from_str(config.as_str()).unwrap();
        let yaml = yaml.as_mapping_mut().unwrap();

        //修改 WebUI

        match yaml.get_mut("external-controller") {
            Some(x) => {
                *x = Value::String(String::from("127.0.0.1:9090"));
            },
            None => {
                yaml.insert(Value::String(String::from("external-controller")), Value::String(String::from("127.0.0.1:9090")));
            }
        }

        let webui_dir = get_current_working_dir().unwrap().join("bin/core/web");

        match yaml.get_mut("external-ui") {
            Some(x) => {
                //TODO: 修改 Web UI 的路径
                *x = Value::String(String::from(webui_dir.to_str().unwrap()));
            },
            None => {
                yaml.insert(Value::String(String::from("external-controller")), Value::String(String::from(webui_dir.to_str().unwrap())));
            }
        }

        //修改 TUN 和 DNS 配置

        let tun_config = "
        enable: true
        stack: system
        auto-route: true
        auto-detect-interface: true
        ";


        //部分配置来自 https://www.xkww3n.cyou/2022/02/08/use-clash-dns-anti-dns-hijacking/
        let dns_config = "
        enable: true
        listen: 0.0.0.0:53
        enhanced-mode: fake-ip
        fake-ip-range: 198.18.0.1/16
        nameserver:
            - https://doh.pub/dns-query
            - https://dns.alidns.com/dns-query
            - '114.114.114.114'
            - '223.5.5.5'
        default-nameserver:
            - 119.29.29.29
            - 223.5.5.5
        fallback:
            - https://1.1.1.1/dns-query
            - https://dns.google/dns-query
            - https://doh.opendns.com/dns-query
            - https://doh.pub/dns-query
        fallback-filter:
            geoip: true
            geoip-code: CN
            ipcidr:
                - 240.0.0.0/4
        ";

        let insert_config = |yaml: &mut Mapping, config: &str, key: &str| {
            let inner_config: Value = serde_yaml::from_str(config).unwrap();
            yaml.insert(Value::String(String::from(key)), inner_config);
        };

        //开启 tun 模式
        match yaml.get("tun") {
            Some(_) => {
                yaml.remove("tun").unwrap();
                insert_config(yaml, tun_config, "tun");
            },
            None => {
                insert_config(yaml, tun_config, "tun");
            }
        }

        match yaml.get("dns") {
            Some(_) => {
                //删除 DNS 配置
                yaml.remove("dns").unwrap();
                insert_config(yaml, dns_config, "dns");
            }
            None => {
                insert_config(yaml, dns_config, "dns");
            }
        }

        
        let yaml_str = serde_yaml::to_string(&yaml).unwrap();
        fs::write("./bin/config.new.yaml", yaml_str).unwrap();
    }
}