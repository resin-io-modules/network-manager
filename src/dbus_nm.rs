use std::collections::HashMap;
use std::net::Ipv4Addr;

use dbus::Path;
use dbus::arg::{Array, Dict, Iter, RefArg, Variant};

use ascii::AsciiStr;

use errors::*;
use dbus_api::{extract, path_to_string, DBusApi, VariantTo, variant_iter_to_vec_u8};
use manager::{Connectivity, NetworkManagerState};
use connection::{ConnectionSettings, ConnectionState, ConnectionConfig};
use ssid::{AsSsidSlice, Ssid};
use device::{DeviceState, DeviceType};
use wifi::{AccessPoint, AccessPointCredentials, NM80211ApFlags, NM80211ApSecurityFlags};

type VariantMap = HashMap<String, Variant<Box<RefArg>>>;

const NM_SERVICE_MANAGER: &str = "org.freedesktop.NetworkManager";

const NM_SERVICE_PATH: &str = "/org/freedesktop/NetworkManager";
const NM_SETTINGS_PATH: &str = "/org/freedesktop/NetworkManager/Settings";

const NM_SERVICE_INTERFACE: &str = "org.freedesktop.NetworkManager";
const NM_SETTINGS_INTERFACE: &str = "org.freedesktop.NetworkManager.Settings";
const NM_CONNECTION_INTERFACE: &str = "org.freedesktop.NetworkManager.Settings.\
                                       Connection";
const NM_ACTIVE_INTERFACE: &str = "org.freedesktop.NetworkManager.Connection.Active";
const NM_DEVICE_INTERFACE: &str = "org.freedesktop.NetworkManager.Device";
const NM_WIRELESS_INTERFACE: &str = "org.freedesktop.NetworkManager.Device.Wireless";
const NM_ACCESS_POINT_INTERFACE: &str = "org.freedesktop.NetworkManager.AccessPoint";

const NM_WEP_KEY_TYPE_PASSPHRASE: u32 = 2;

const UNKNOWN_CONNECTION: &str = "org.freedesktop.NetworkManager.UnknownConnection";
const METHOD_RETRY_ERROR_NAMES: &[&str; 1] = &[UNKNOWN_CONNECTION];

pub struct DBusNetworkManager {
    dbus: DBusApi,
}

impl DBusNetworkManager {
    pub fn new(method_timeout: Option<u64>) -> Self {
        DBusNetworkManager {
            dbus: DBusApi::new(NM_SERVICE_MANAGER, METHOD_RETRY_ERROR_NAMES, method_timeout),
        }
    }

    pub fn method_timeout(&self) -> u64 {
        self.dbus.method_timeout()
    }

    pub fn get_state(&self) -> Result<NetworkManagerState> {
        let response = self.dbus
            .call(NM_SERVICE_PATH, NM_SERVICE_INTERFACE, "state")?;

        let state: u32 = self.dbus.extract(&response)?;

        Ok(NetworkManagerState::from(state))
    }

    pub fn check_connectivity(&self) -> Result<Connectivity> {
        let response = self.dbus
            .call(NM_SERVICE_PATH, NM_SERVICE_INTERFACE, "CheckConnectivity")?;

        let connectivity: u32 = self.dbus.extract(&response)?;

        Ok(Connectivity::from(connectivity))
    }

    pub fn is_wireless_enabled(&self) -> Result<bool> {
        self.dbus
            .property(NM_SERVICE_PATH, NM_SERVICE_INTERFACE, "WirelessEnabled")
    }

    pub fn is_networking_enabled(&self) -> Result<bool> {
        self.dbus
            .property(NM_SERVICE_PATH, NM_SERVICE_INTERFACE, "NetworkingEnabled")
    }

    pub fn list_connections(&self) -> Result<Vec<String>> {
        let response = self.dbus
            .call(NM_SETTINGS_PATH, NM_SETTINGS_INTERFACE, "ListConnections")?;

        let array: Array<Path, _> = self.dbus.extract(&response)?;

        Ok(array.map(|e| e.to_string()).collect())
    }

    pub fn get_active_connections(&self) -> Result<Vec<String>> {
        self.dbus
            .property(NM_SERVICE_PATH, NM_SERVICE_INTERFACE, "ActiveConnections")
    }

    pub fn get_active_connection_path(&self, path: &str) -> Option<String> {
        self.dbus
            .property(path, NM_ACTIVE_INTERFACE, "Connection")
            .ok()
    }

    pub fn get_connection_state(&self, path: &str) -> Result<ConnectionState> {
        let state: i64 = match self.dbus.property(path, NM_ACTIVE_INTERFACE, "State") {
            Ok(state) => state,
            Err(_) => return Ok(ConnectionState::Unknown),
        };

        Ok(ConnectionState::from(state))
    }

    pub fn get_connection_settings(&self, path: &str) -> Result<ConnectionSettings> {
        let response = self.dbus
            .call(path, NM_CONNECTION_INTERFACE, "GetSettings")?;

        let dict: Dict<&str, Dict<&str, Variant<Iter>, _>, _> = self.dbus.extract(&response)?;

        let mut kind = String::new();
        let mut id = String::new();
        let mut uuid = String::new();
        let mut ssid = Ssid::new();
        let mut mode = String::new();

        for (_, v1) in dict {
            for (k2, mut v2) in v1 {
                match k2 {
                    "id" => {
                        id = extract::<String>(&mut v2)?;
                    },
                    "uuid" => {
                        uuid = extract::<String>(&mut v2)?;
                    },
                    "type" => {
                        kind = extract::<String>(&mut v2)?;
                    },
                    "ssid" => {
                        ssid = Ssid::from_bytes(variant_iter_to_vec_u8(&mut v2)?)?;
                    },
                    "mode" => {
                        mode = extract::<String>(&mut v2)?;
                    },
                    _ => {},
                }
            }
        }

        Ok(ConnectionSettings {
            kind: kind,
            id: id,
            uuid: uuid,
            ssid: ssid,
            mode: mode,
        })
    }

    pub fn get_active_connection_devices(&self, path: &str) -> Result<Vec<String>> {
        self.dbus.property(path, NM_ACTIVE_INTERFACE, "Devices")
    }

    pub fn delete_connection(&self, path: &str) -> Result<()> {
        self.dbus.call(path, NM_CONNECTION_INTERFACE, "Delete")?;

        Ok(())
    }

    pub fn activate_connection(&self, path: &str) -> Result<()> {
        self.dbus.call_with_args(
            NM_SERVICE_PATH,
            NM_SERVICE_INTERFACE,
            "ActivateConnection",
            &[
                &Path::new(path)? as &RefArg,
                &Path::new("/")? as &RefArg,
                &Path::new("/")? as &RefArg,
            ],
        )?;

        Ok(())
    }

    pub fn deactivate_connection(&self, path: &str) -> Result<()> {
        self.dbus.call_with_args(
            NM_SERVICE_PATH,
            NM_SERVICE_INTERFACE,
            "DeactivateConnection",
            &[&Path::new(path)? as &RefArg],
        )?;

        Ok(())
    }

    fn connect_to_access_point_impl(
        &self,
        device_path: &str,
        access_point: &AccessPoint,
        credentials: &AccessPointCredentials,
        config_maybe: Option<&ConnectionConfig>,
    ) -> Result<(String, String)> {
        let mut settings: HashMap<String, VariantMap> = HashMap::new();

        let mut wireless: VariantMap = HashMap::new();
        add_val(
            &mut wireless,
            "ssid",
            access_point.ssid().as_bytes().to_vec(),
        );
        settings.insert("802-11-wireless".to_string(), wireless);

        match *credentials {
            AccessPointCredentials::Wep { ref passphrase } => {
                let mut security_settings: VariantMap = HashMap::new();

                add_val(
                    &mut security_settings,
                    "wep-key-type",
                    NM_WEP_KEY_TYPE_PASSPHRASE,
                );
                add_str(
                    &mut security_settings,
                    "wep-key0",
                    verify_ascii_password(passphrase)?,
                );

                settings.insert("802-11-wireless-security".to_string(), security_settings);
            },
            AccessPointCredentials::Wpa { ref passphrase } => {
                let mut security_settings: VariantMap = HashMap::new();

                add_str(&mut security_settings, "key-mgmt", "wpa-psk");
                add_str(
                    &mut security_settings,
                    "psk",
                    verify_ascii_password(passphrase)?,
                );

                settings.insert("802-11-wireless-security".to_string(), security_settings);
            },
            AccessPointCredentials::Enterprise {
                ref identity,
                ref passphrase,
            } => {
                let mut security_settings: VariantMap = HashMap::new();

                add_str(&mut security_settings, "key-mgmt", "wpa-eap");

                let mut eap: VariantMap = HashMap::new();
                add_val(&mut eap, "eap", vec!["peap".to_string()]);
                add_str(&mut eap, "identity", identity as &str);
                add_str(&mut eap, "password", passphrase as &str);
                add_str(&mut eap, "phase2-auth", "mschapv2");

                settings.insert("802-11-wireless-security".to_string(), security_settings);
                settings.insert("802-1x".to_string(), eap);
            },
            AccessPointCredentials::None => {},
        };

        // Extra config
        if let Some(config) = config_maybe {
            // Add DNS information
            if let Some(dns4) = config.dns4 {
                let mut ipv4_variant: VariantMap = HashMap::new();
                add_val(&mut ipv4_variant, "ignore-auto-dns", true);
                add_str(&mut ipv4_variant, "method", "auto");
                add_val(
                    &mut ipv4_variant, 
                    "dns",
                    dns4.iter()
                        .map(|x| u32::from(x.clone()))
                        .collect::<Vec<u32>>()
                );
                settings.insert("ipv4".to_string(), ipv4_variant);
            };
        };

        let response = self.dbus.call_with_args(
            NM_SERVICE_PATH,
            NM_SERVICE_INTERFACE,
            "AddAndActivateConnection",
            &[
                &settings as &RefArg,
                &Path::new(device_path.to_string())? as &RefArg,
                &Path::new(access_point.path.to_string())? as &RefArg,
            ],
        )?;

        let (conn_path, active_connection): (Path, Path) = self.dbus.extract_two(&response)?;

        Ok((
            path_to_string(&conn_path)?,
            path_to_string(&active_connection)?,
        ))

    }

    pub fn connect_to_access_point(
        &self,
        device_path: &str,
        access_point: &AccessPoint,
        credentials: &AccessPointCredentials,
    ) -> Result<(String, String)> {
        self.connect_to_access_point_impl(
            device_path,
            access_point,
            credentials,
            None,
        )
    }

    pub fn connect_to_access_point_with_config(
        &self,
        device_path: &str,
        access_point: &AccessPoint,
        credentials: &AccessPointCredentials,
        config: &ConnectionConfig,
    ) -> Result<(String, String)> {
        self.connect_to_access_point_impl(
            device_path,
            access_point,
            credentials,
            Some(config),
        )
    }

    pub fn create_hotspot<T>(
        &self,
        device_path: &str,
        interface: &str,
        ssid: &T,
        password: Option<&str>,
        address: Option<Ipv4Addr>,
    ) -> Result<(String, String)>
    where
        T: AsSsidSlice + ?Sized,
    {
        let ssid = ssid.as_ssid_slice()?;
        let ssid_vec = ssid.as_bytes().to_vec();

        let mut wireless: VariantMap = HashMap::new();
        add_val(&mut wireless, "ssid", ssid_vec);
        add_str(&mut wireless, "band", "bg");
        add_val(&mut wireless, "hidden", false);
        add_str(&mut wireless, "mode", "ap");

        let mut connection: VariantMap = HashMap::new();
        add_val(&mut connection, "autoconnect", false);
        if let Ok(ssid_str) = ssid.as_str() {
            add_str(&mut connection, "id", ssid_str);
        }
        add_str(&mut connection, "interface-name", interface);
        add_str(&mut connection, "type", "802-11-wireless");

        let mut ipv4: VariantMap = HashMap::new();
        if let Some(address) = address {
            add_str(&mut ipv4, "method", "manual");

            let mut addr_map: VariantMap = HashMap::new();
            add_str(&mut addr_map, "address", format!("{}", address));
            add_val(&mut addr_map, "prefix", 24_u32);

            add_val(&mut ipv4, "address-data", vec![addr_map]);
        } else {
            add_str(&mut ipv4, "method", "shared");
        }

        let mut settings: HashMap<String, VariantMap> = HashMap::new();

        if let Some(password) = password {
            add_str(&mut wireless, "security", "802-11-wireless-security");

            let mut security: VariantMap = HashMap::new();
            add_str(&mut security, "key-mgmt", "wpa-psk");
            add_str(&mut security, "psk", verify_ascii_password(password)?);

            settings.insert("802-11-wireless-security".to_string(), security);
        }

        settings.insert("802-11-wireless".to_string(), wireless);
        settings.insert("connection".to_string(), connection);
        settings.insert("ipv4".to_string(), ipv4);

        let response = self.dbus.call_with_args(
            NM_SERVICE_PATH,
            NM_SERVICE_INTERFACE,
            "AddAndActivateConnection",
            &[
                &settings as &RefArg,
                &Path::new(device_path)? as &RefArg,
                &Path::new("/")? as &RefArg,
            ],
        )?;

        let (conn_path, active_connection): (Path, Path) = self.dbus.extract_two(&response)?;

        Ok((
            path_to_string(&conn_path)?,
            path_to_string(&active_connection)?,
        ))
    }

    pub fn get_devices(&self) -> Result<Vec<String>> {
        self.dbus
            .property(NM_SERVICE_PATH, NM_SERVICE_INTERFACE, "Devices")
    }

    pub fn get_device_by_interface(&self, interface: &str) -> Result<String> {
        let response = self.dbus.call_with_args(
            NM_SERVICE_PATH,
            NM_SERVICE_INTERFACE,
            "GetDeviceByIpIface",
            &[&interface.to_string() as &RefArg],
        )?;

        let path: Path = self.dbus.extract(&response)?;

        path_to_string(&path)
    }

    pub fn get_device_interface(&self, path: &str) -> Result<String> {
        self.dbus.property(path, NM_DEVICE_INTERFACE, "Interface")
    }

    pub fn get_device_type(&self, path: &str) -> Result<DeviceType> {
        self.dbus.property(path, NM_DEVICE_INTERFACE, "DeviceType")
    }

    pub fn get_device_state(&self, path: &str) -> Result<DeviceState> {
        self.dbus.property(path, NM_DEVICE_INTERFACE, "State")
    }

    pub fn connect_device(&self, path: &str) -> Result<()> {
        self.dbus.call_with_args(
            NM_SERVICE_PATH,
            NM_SERVICE_INTERFACE,
            "ActivateConnection",
            &[
                &Path::new("/")? as &RefArg,
                &Path::new(path)? as &RefArg,
                &Path::new("/")? as &RefArg,
            ],
        )?;

        Ok(())
    }

    pub fn disconnect_device(&self, path: &str) -> Result<()> {
        self.dbus.call(path, NM_DEVICE_INTERFACE, "Disconnect")?;

        Ok(())
    }

    pub fn request_access_point_scan(&self, path: &str) -> Result<()> {
        let options: VariantMap = HashMap::new();
        self.dbus.call_with_args(
            path,
            NM_WIRELESS_INTERFACE,
            "RequestScan",
            &[&options as &RefArg],
        )?;

        Ok(())
    }

    pub fn get_device_access_points(&self, path: &str) -> Result<Vec<String>> {
        self.dbus
            .property(path, NM_WIRELESS_INTERFACE, "AccessPoints")
    }

    pub fn get_access_point_ssid(&self, path: &str) -> Option<Ssid> {
        if let Ok(ssid_vec) = self.dbus
            .property::<Vec<u8>>(path, NM_ACCESS_POINT_INTERFACE, "Ssid")
        {
            Ssid::from_bytes(ssid_vec).ok()
        } else {
            None
        }
    }

    pub fn get_access_point_strength(&self, path: &str) -> Result<u32> {
        self.dbus
            .property(path, NM_ACCESS_POINT_INTERFACE, "Strength")
    }

    pub fn get_access_point_flags(&self, path: &str) -> Result<NM80211ApFlags> {
        self.dbus.property(path, NM_ACCESS_POINT_INTERFACE, "Flags")
    }

    pub fn get_access_point_wpa_flags(&self, path: &str) -> Result<NM80211ApSecurityFlags> {
        self.dbus
            .property(path, NM_ACCESS_POINT_INTERFACE, "WpaFlags")
    }

    pub fn get_access_point_rsn_flags(&self, path: &str) -> Result<NM80211ApSecurityFlags> {
        self.dbus
            .property(path, NM_ACCESS_POINT_INTERFACE, "RsnFlags")
    }
}

impl VariantTo<DeviceType> for DBusApi {
    fn variant_to(value: &Variant<Box<RefArg>>) -> Option<DeviceType> {
        value.0.as_i64().map(DeviceType::from)
    }
}

impl VariantTo<DeviceState> for DBusApi {
    fn variant_to(value: &Variant<Box<RefArg>>) -> Option<DeviceState> {
        value.0.as_i64().map(DeviceState::from)
    }
}

impl VariantTo<NM80211ApFlags> for DBusApi {
    fn variant_to(value: &Variant<Box<RefArg>>) -> Option<NM80211ApFlags> {
        value
            .0
            .as_i64()
            .and_then(|v| NM80211ApFlags::from_bits(v as u32))
    }
}

impl VariantTo<NM80211ApSecurityFlags> for DBusApi {
    fn variant_to(value: &Variant<Box<RefArg>>) -> Option<NM80211ApSecurityFlags> {
        value
            .0
            .as_i64()
            .and_then(|v| NM80211ApSecurityFlags::from_bits(v as u32))
    }
}

pub fn add_val<K, V>(map: &mut VariantMap, key: K, value: V)
where
    K: Into<String>,
    V: RefArg + 'static,
{
    map.insert(key.into(), Variant(Box::new(value)));
}

pub fn add_str<K, V>(map: &mut VariantMap, key: K, value: V)
where
    K: Into<String>,
    V: Into<String>,
{
    map.insert(key.into(), Variant(Box::new(value.into())));
}

fn verify_ascii_password(password: &str) -> Result<&str> {
    match AsciiStr::from_ascii(password) {
        Err(e) => Err(e).chain_err(|| ErrorKind::PreSharedKey("Not an ASCII password".into())),
        Ok(p) => {
            if p.len() < 8 {
                bail!(ErrorKind::PreSharedKey(format!(
                    "Password length should be at least 8 characters: {} len",
                    p.len()
                )))
            } else if p.len() > 64 {
                bail!(ErrorKind::PreSharedKey(format!(
                    "Password length should not exceed 64: {} len",
                    p.len()
                )))
            } else {
                Ok(password)
            }
        },
    }
}
