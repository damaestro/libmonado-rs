use dlopen2::wrapper::Container;
use dlopen2::wrapper::WrapperApi;
use flagset::FlagSet;
use semver::{Version, VersionReq};
use std::ffi::c_char;
use std::ffi::c_void;
use std::ffi::CStr;
use std::ffi::CString;
use std::ffi::OsStr;
use std::fmt::Debug;
use std::vec;

#[repr(i32)]
#[doc = " Result codes for operations, negative are errors, zero or positives are\n success."]
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub enum MndResult {
	Success = 0,
	ErrorInvalidVersion = -1,
	ErrorInvalidValue = -2,
	ErrorConnectingFailed = -3,
	ErrorOperationFailed = -4,
}
impl MndResult {
	pub fn to_result(self) -> Result<(), MndResult> {
		if self == MndResult::Success {
			Ok(())
		} else {
			Err(self)
		}
	}
}

flagset::flags! {
	#[doc = " Bitflags for client application state."]
	pub enum ClientState: u32 {
		ClientPrimaryApp = 1,
		ClientSessionActive = 2,
		ClientSessionVisible = 4,
		ClientSessionFocused = 8,
		ClientSessionOverlay = 16,
		ClientIoActive = 32,
	}
}

#[doc = " Opaque type for libmonado state"]
pub type MndRootPtr = *mut c_void;

#[derive(WrapperApi)]
pub struct MonadoApi {
	mnd_api_get_version:
		unsafe extern "C" fn(out_major: *mut u32, out_minor: *mut u32, out_patch: *mut u32),
	mnd_root_create: unsafe extern "C" fn(out_root: *mut MndRootPtr) -> MndResult,
	mnd_root_destroy: unsafe extern "C" fn(out_root: *mut MndRootPtr),
	mnd_root_update_client_list: unsafe extern "C" fn(root: MndRootPtr) -> MndResult,
	mnd_root_get_number_clients:
		unsafe extern "C" fn(root: MndRootPtr, out_num: *mut u32) -> MndResult,
	mnd_root_get_client_id_at_index:
		unsafe extern "C" fn(root: MndRootPtr, index: u32, out_client_id: *mut u32) -> MndResult,
	mnd_root_get_client_name: unsafe extern "C" fn(
		root: MndRootPtr,
		client_id: u32,
		out_name: *mut *const ::std::os::raw::c_char,
	) -> MndResult,
	mnd_root_get_client_state:
		unsafe extern "C" fn(root: MndRootPtr, client_id: u32, out_flags: *mut u32) -> MndResult,
	mnd_root_set_client_primary:
		unsafe extern "C" fn(root: MndRootPtr, client_id: u32) -> MndResult,
	mnd_root_set_client_focused:
		unsafe extern "C" fn(root: MndRootPtr, client_id: u32) -> MndResult,
	mnd_root_toggle_client_io_active:
		unsafe extern "C" fn(root: MndRootPtr, client_id: u32) -> MndResult,
	mnd_root_get_device_count:
		unsafe extern "C" fn(root: MndRootPtr, out_device_count: *mut u32) -> MndResult,
	mnd_root_get_device_info: unsafe extern "C" fn(
		root: MndRootPtr,
		device_index: u32,
		out_device_id: *mut u32,
		out_dev_name: *mut *const ::std::os::raw::c_char,
	) -> MndResult,
	mnd_root_get_device_from_role: unsafe extern "C" fn(
		root: MndRootPtr,
		role_name: *const ::std::os::raw::c_char,
		out_device_id: *mut i32,
	) -> MndResult,
}

fn crate_api_version() -> VersionReq {
	VersionReq::parse("=1.0.0").unwrap()
}
fn get_api_version(api: &Container<MonadoApi>) -> Version {
	let mut major = 0;
	let mut minor = 0;
	let mut patch = 0;
	unsafe { api.mnd_api_get_version(&mut major, &mut minor, &mut patch) };

	Version::new(major as u64, minor as u64, patch as u64)
}

pub struct Monado {
	api: Container<MonadoApi>,
	root: MndRootPtr,
}
impl Monado {
	pub fn create<S: AsRef<OsStr>>(libmonado_so: S) -> Result<Self, MndResult> {
		let api = unsafe { Container::<MonadoApi>::load(libmonado_so) }
			.map_err(|_| MndResult::ErrorConnectingFailed)?;
		if !crate_api_version().matches(&get_api_version(&api)) {
			return Err(MndResult::ErrorInvalidVersion);
		}
		let mut root = std::ptr::null_mut();
		unsafe {
			api.mnd_root_create(&mut root).to_result()?;
		}
		Ok(Monado { api, root })
	}

	pub fn get_api_version(&self) -> Version {
		get_api_version(&self.api)
	}

	pub fn clients<'m>(&'m self) -> Result<impl IntoIterator<Item = Client<'m>>, MndResult> {
		unsafe {
			self.api
				.mnd_root_update_client_list(self.root)
				.to_result()?
		};
		let mut client_count = 0;
		unsafe {
			self.api
				.mnd_root_get_number_clients(self.root, &mut client_count)
				.to_result()?
		};
		let mut clients: Vec<Option<Client>> = vec::from_elem(None, client_count as usize);
		for (index, client) in clients.iter_mut().enumerate() {
			let mut id = 0;
			unsafe {
				self.api
					.mnd_root_get_client_id_at_index(self.root, index as u32, &mut id)
					.to_result()?
			};
			client.replace(Client { monado: self, id });
		}
		Ok(clients.into_iter().flatten())
	}

	// Get device id from role name
	//
	// @param root Opaque libmonado state
	// @param role_name Name of the role
	// @param out_device_id Pointer to populate with device id
	pub fn device_from_role<'m>(&'m self, role_name: &str) -> Result<Device<'m>, MndResult> {
		let c_name = CString::new(role_name).unwrap();
		let mut device_id = -1;

		unsafe {
			self.api
				.mnd_root_get_device_from_role(self.root, c_name.as_ptr(), &mut device_id)
				.to_result()?
		};
		let mut id = 0;
		let mut c_name: *const c_char = std::ptr::null_mut();
		unsafe {
			self.api
				.mnd_root_get_device_info(self.root, device_id as u32, &mut id, &mut c_name)
				.to_result()?
		};
		let name = unsafe {
			CStr::from_ptr(c_name)
				.to_str()
				.map_err(|_| MndResult::ErrorInvalidValue)?
				.to_owned()
		};

		Ok(Device {
			_monado: self,
			id,
			name,
		})
	}

	pub fn devices<'m>(&'m self) -> Result<impl IntoIterator<Item = Device<'m>>, MndResult> {
		let mut device_count = 0;
		unsafe {
			self.api
				.mnd_root_get_device_count(self.root, &mut device_count)
				.to_result()?
		};
		let mut devices: Vec<Option<Device>> = vec::from_elem(None, device_count as usize);
		for (index, device) in devices.iter_mut().enumerate() {
			let mut id = 0;
			let mut c_name: *const c_char = std::ptr::null_mut();
			unsafe {
				self.api
					.mnd_root_get_device_info(self.root, index as u32, &mut id, &mut c_name)
					.to_result()?
			};
			let name = unsafe {
				CStr::from_ptr(c_name)
					.to_str()
					.map_err(|_| MndResult::ErrorInvalidValue)?
					.to_owned()
			};
			device.replace(Device {
				_monado: self,
				id,
				name,
			});
		}
		Ok(devices.into_iter().flatten())
	}
}
impl Drop for Monado {
	fn drop(&mut self) {
		unsafe { self.api.mnd_root_destroy(&mut self.root) }
	}
}

#[derive(Clone)]
pub struct Client<'m> {
	monado: &'m Monado,
	id: u32,
}
impl Client<'_> {
	pub fn name(&mut self) -> Result<String, MndResult> {
		let mut string = std::ptr::null();
		unsafe {
			self.monado
				.api
				.mnd_root_get_client_name(self.monado.root, self.id, &mut string)
				.to_result()?
		};
		let c_string = unsafe { CStr::from_ptr(string) };
		c_string
			.to_str()
			.map_err(|_| MndResult::ErrorInvalidValue)
			.map(ToString::to_string)
	}
	pub fn state(&mut self) -> Result<FlagSet<ClientState>, MndResult> {
		let mut state = 0;
		unsafe {
			self.monado
				.api
				.mnd_root_get_client_state(self.monado.root, self.id, &mut state)
				.to_result()?
		};
		Ok(unsafe { FlagSet::new_unchecked(state) })
	}
	pub fn set_primary(&mut self) -> Result<(), MndResult> {
		unsafe {
			self.monado
				.api
				.mnd_root_set_client_primary(self.monado.root, self.id)
				.to_result()
		}
	}
	pub fn set_focused(&mut self) -> Result<(), MndResult> {
		unsafe {
			self.monado
				.api
				.mnd_root_set_client_focused(self.monado.root, self.id)
				.to_result()
		}
	}
	pub fn set_io_active(&mut self, active: bool) -> Result<(), MndResult> {
		let state = self.state()?;
		if state.contains(ClientState::ClientIoActive) != active {
			unsafe {
				self.monado
					.api
					.mnd_root_toggle_client_io_active(self.monado.root, self.id)
					.to_result()?;
			}
		}
		Ok(())
	}
}

#[derive(Clone)]
pub struct Device<'m> {
	_monado: &'m Monado,
	pub id: u32,
	pub name: String,
}
impl Debug for Device<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Device")
			.field("id", &self.id)
			.field("name", &self.name)
			.finish()
	}
}

// #[test]
// fn test_dump_info() {
// 	dbg!(get_api_version());
// 	let monado = Monado::create().unwrap();
// 	for mut client in monado.clients().unwrap() {
// 		println!(
// 			"Client name is {} and state is {:?}",
// 			client.name().unwrap(),
// 			client.state().unwrap()
// 		)
// 	}
// }