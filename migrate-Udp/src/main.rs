use os_socketaddr::OsSocketAddr;
use std::mem;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::os::raw::c_void;
use std::ptr;
use std::thread;
use std::time::Duration;
use windows_sys::Win32::Foundation::NO_ERROR;
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetBestRoute2, NotifyRouteChange2, NotifyUnicastIpAddressChange, MIB_IPFORWARD_ROW2,
    MIB_NOTIFICATION_TYPE, MIB_UNICASTIPADDRESS_ROW,
};
use windows_sys::Win32::Networking::WinSock::{AF_UNSPEC, SOCKADDR_INET};

extern "system" fn route_change_callback(
    _callercontext: *const c_void,
    row: *const MIB_IPFORWARD_ROW2,
    notificationtype: MIB_NOTIFICATION_TYPE,
) {
    if row != ptr::null() {
        let (prefix, prefix_len) = unsafe {
            let prefix = OsSocketAddr::from_raw_parts(
                &(*row).DestinationPrefix.Prefix as *const _ as *const u8,
                mem::size_of::<SOCKADDR_INET>(),
            )
            .into_addr()
            .unwrap()
            .ip();
            (prefix, (*row).DestinationPrefix.PrefixLength)
        };
        match notificationtype {
            0 => {
                eprintln!("[CHANGE] prefix: {:?}/{}", prefix, prefix_len);
            }
            1 => {
                eprintln!("[ADD] prefix: {:?}/{}", prefix, prefix_len);
            }
            2 => {
                eprintln!("[DELETE] prefix: {:?}/{}", prefix, prefix_len);
            }
            _ => {}
        }

        let dest_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(172, 29, 206, 88)), 0);
        let dest_addr: OsSocketAddr = dest_addr.into();
        let mut best_route: MIB_IPFORWARD_ROW2 = unsafe { mem::zeroed() };
        let mut best_src_addr = OsSocketAddr::new();
        let res = unsafe {
            GetBestRoute2(
                ptr::null(),
                0,
                ptr::null(),
                dest_addr.as_ptr() as *const _ as *const SOCKADDR_INET,
                0,
                &mut best_route,
                best_src_addr.as_mut_ptr() as *mut _ as *mut SOCKADDR_INET,
            )
        };
        if res == NO_ERROR as i32 {
            let nexthop = unsafe {
                OsSocketAddr::from_raw_parts(
                    &best_route.NextHop as *const _ as *const u8,
                    std::mem::size_of::<SOCKADDR_INET>(),
                )
            };
            eprintln!(" [BEST] nexthop: {:?}", nexthop);
            eprintln!(" [BEST] src_addr: {:?}", best_src_addr);
        }
    }
}

extern "system" fn address_change_callback(
    _callercontext: *const c_void,
    row: *const MIB_UNICASTIPADDRESS_ROW,
    notificationtype: MIB_NOTIFICATION_TYPE,
) {
    if row != ptr::null() {
        let address = unsafe {
            OsSocketAddr::from_raw_parts(
                &(*row).Address as *const _ as *const u8,
                mem::size_of::<SOCKADDR_INET>(),
            )
            .into_addr()
            .unwrap()
            .ip()
        };
        match notificationtype {
            0 => {
                eprintln!("[CHANGE] address: {:?}", address);
            }
            1 => {
                eprintln!("[ADD] address: {:?}", address);
            }
            2 => {
                eprintln!("[DELETE] address: {:?}", address);
            }
            _ => {}
        }
    }
}

fn main() -> std::io::Result<()> {
    let mut route_change_handle = 0;
    unsafe {
        NotifyRouteChange2(
            AF_UNSPEC as u16,
            Some(route_change_callback),
            ptr::null(),
            1,
            &mut route_change_handle,
        );
    }
    let mut address_change_handle = 0;
    unsafe {
        NotifyUnicastIpAddressChange(
            AF_UNSPEC as u16,
            Some(address_change_callback),
            ptr::null(),
            0,
            &mut address_change_handle,
        );
    }

    thread::sleep(Duration::MAX);
    Ok(())
}
