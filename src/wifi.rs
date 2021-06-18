use std::rc::Rc;
use std::net::Ipv4Addr;

use errors::*;
use dbus_nm::DBusNetworkManager;

use connection::{connect_to_access_point, connect_to_access_point_with_config, 
    create_hotspot, Connection, ConnectionState, ConnectionConfig};
use device::{Device, PathGetter};
use ssid::{AsSsidSlice, Ssid, SsidSlice};

pub struct WiFiDevice<'a> {
    dbus_manager: Rc<DBusNetworkManager>,
    device: &'a Device,
}

impl<'a> WiFiDevice<'a> {
    /// Get the list of access points visible to this device.
    ///
    /// # Examples
    ///
    /// ```
    /// use network_manager::{NetworkManager, DeviceType};
    /// let manager = NetworkManager::new();
    /// let devices = manager.get_devices().unwrap();
    /// let i = devices.iter().position(|ref d| *d.device_type() == DeviceType::WiFi).unwrap();
    /// let device = devices[i].as_wifi_device().unwrap();
    /// device.request_scan()?;
    /// let access_points = device.get_access_points().unwrap();
    /// println!("{:?}", access_points);
    /// ```
    pub fn get_access_points(&self) -> Result<Vec<AccessPoint>> {
        let mut access_points = Vec::new();

        let paths = self.dbus_manager
            .get_device_access_points(self.device.path())?;

        for path in paths {
            if let Some(access_point) = get_access_point(&self.dbus_manager, &path)? {
                access_points.push(access_point);
            }
        }

        access_points.sort_by_key(|ap| ap.strength);
        access_points.reverse();

        Ok(access_points)
    }

    pub fn request_scan(&self) -> Result<()> {
        self.dbus_manager
            .request_access_point_scan(self.device.path())?;
        Ok(())
    }

    pub fn connect(
        &self,
        access_point: &AccessPoint,
        credentials: &AccessPointCredentials,
    ) -> Result<(Connection, ConnectionState)> {
        connect_to_access_point(
            &self.dbus_manager,
            self.device.path(),
            access_point,
            credentials,
        )
    }

    pub fn connect_with_config(
        &self,
        access_point: &AccessPoint,
        credentials: &AccessPointCredentials,
        config: &ConnectionConfig,
    ) -> Result<(Connection, ConnectionState)> {
        connect_to_access_point_with_config(
            &self.dbus_manager,
            self.device.path(),
            access_point,
            credentials,
            config
        )

    }

    pub fn create_hotspot<T>(
        &self,
        ssid: &T,
        password: Option<&str>,
        address: Option<Ipv4Addr>,
    ) -> Result<(Connection, ConnectionState)>
    where
        T: AsSsidSlice + ?Sized,
    {
        create_hotspot(
            &self.dbus_manager,
            self.device.path(),
            self.device.interface(),
            ssid,
            password,
            address,
        )
    }
}

#[derive(Debug)]
pub struct AccessPoint {
    pub path: String,
    pub ssid: Ssid,
    pub strength: u32,
    pub security: Security,
}

impl AccessPoint {
    pub fn ssid(&self) -> &SsidSlice {
        &self.ssid
    }
}

bitflags! {
    pub struct Security: u32 {
        const NONE         = 0b0000_0000;
        const WEP          = 0b0000_0001;
        const WPA          = 0b0000_0010;
        const WPA2         = 0b0000_0100;
        const ENTERPRISE   = 0b0000_1000;
    }
}

#[derive(Debug)]
pub enum AccessPointCredentials {
    None,
    Wep {
        passphrase: String,
    },
    Wpa {
        passphrase: String,
    },
    Enterprise {
        identity: String,
        passphrase: String,
    },
}

bitflags! {
    pub struct NM80211ApFlags: u32 {
        // access point has no special capabilities
        const AP_FLAGS_NONE                  = 0x0000_0000;
        // access point requires authentication and encryption (usually means WEP)
        const AP_FLAGS_PRIVACY               = 0x0000_0001;
        // access point supports some WPS method
        const AP_FLAGS_WPS                   = 0x0000_0002;
        // access point supports push-button WPS
        const AP_FLAGS_WPS_PBC               = 0x0000_0004;
        // access point supports PIN-based WPS
        const AP_FLAGS_WPS_PIN               = 0x0000_0008;
    }
}

bitflags! {
    pub struct NM80211ApSecurityFlags: u32 {
         // the access point has no special security requirements
        const AP_SEC_NONE                    = 0x0000_0000;
        // 40/64-bit WEP is supported for pairwise/unicast encryption
        const AP_SEC_PAIR_WEP40              = 0x0000_0001;
        // 104/128-bit WEP is supported for pairwise/unicast encryption
        const AP_SEC_PAIR_WEP104             = 0x0000_0002;
        // TKIP is supported for pairwise/unicast encryption
        const AP_SEC_PAIR_TKIP               = 0x0000_0004;
        // AES/CCMP is supported for pairwise/unicast encryption
        const AP_SEC_PAIR_CCMP               = 0x0000_0008;
        // 40/64-bit WEP is supported for group/broadcast encryption
        const AP_SEC_GROUP_WEP40             = 0x0000_0010;
        // 104/128-bit WEP is supported for group/broadcast encryption
        const AP_SEC_GROUP_WEP104            = 0x0000_0020;
        // TKIP is supported for group/broadcast encryption
        const AP_SEC_GROUP_TKIP              = 0x0000_0040;
        // AES/CCMP is supported for group/broadcast encryption
        const AP_SEC_GROUP_CCMP              = 0x0000_0080;
        // WPA/RSN Pre-Shared Key encryption is supported
        const AP_SEC_KEY_MGMT_PSK            = 0x0000_0100;
        // 802.1x authentication and key management is supported
        const AP_SEC_KEY_MGMT_802_1X         = 0x0000_0200;
    }
}

pub fn new_wifi_device<'a>(
    dbus_manager: &Rc<DBusNetworkManager>,
    device: &'a Device,
) -> WiFiDevice<'a> {
    WiFiDevice {
        dbus_manager: Rc::clone(dbus_manager),
        device: device,
    }
}

fn get_access_point(manager: &DBusNetworkManager, path: &str) -> Result<Option<AccessPoint>> {
    if let Some(ssid) = manager.get_access_point_ssid(path) {
        let strength = manager.get_access_point_strength(path)?;

        let security = get_access_point_security(manager, path)?;

        let access_point = AccessPoint {
            path: path.to_string(),
            ssid: ssid,
            strength: strength,
            security: security,
        };

        Ok(Some(access_point))
    } else {
        Ok(None)
    }
}

fn get_access_point_security(manager: &DBusNetworkManager, path: &str) -> Result<Security> {
    let flags = manager.get_access_point_flags(path)?;

    let wpa_flags = manager.get_access_point_wpa_flags(path)?;

    let rsn_flags = manager.get_access_point_rsn_flags(path)?;

    let mut security = Security::NONE;

    if flags.contains(NM80211ApFlags::AP_FLAGS_PRIVACY)
        && wpa_flags == NM80211ApSecurityFlags::AP_SEC_NONE
        && rsn_flags == NM80211ApSecurityFlags::AP_SEC_NONE
    {
        security |= Security::WEP;
    }

    if wpa_flags != NM80211ApSecurityFlags::AP_SEC_NONE {
        security |= Security::WPA;
    }

    if rsn_flags != NM80211ApSecurityFlags::AP_SEC_NONE {
        security |= Security::WPA2;
    }

    if wpa_flags.contains(NM80211ApSecurityFlags::AP_SEC_KEY_MGMT_802_1X)
        || rsn_flags.contains(NM80211ApSecurityFlags::AP_SEC_KEY_MGMT_802_1X)
    {
        security |= Security::ENTERPRISE;
    }

    Ok(security)
}
