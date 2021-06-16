#![cfg_attr(not(feature = "std"), no_std)]

/// Edit this file to define custom logic or remove it if it is not needed.
/// Learn more about FRAME and the core library of Substrate FRAME pallets:
/// <https://substrate.dev/docs/en/knowledgebase/runtime/frame>
pub use pallet::*;
// use frame_system::{
// 	self as system,
// };

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;


use sp_core::crypto::KeyTypeId;


/// Defines application identifier for crypto keys of this module.
///
/// Every module that deals with signatures needs to declare its unique identifier for
/// its crypto keys.
/// When offchain worker is signing transactions it's going to request keys of type
/// `KeyTypeId` from the keystore and use the ones it finds to sign the transaction.
/// The keys can be inserted manually via RPC (see `author_insertKey`).
/// ocpf mean off-chain worker price fetch
pub const KEY_TYPE: KeyTypeId = KeyTypeId(*b"ocpf");


/// Based on the above `KeyTypeId` we need to generate a pallet-specific crypto type wrappers.
/// We can use from supported crypto kinds (`sr25519`, `ed25519` and `ecdsa`) and augment
/// the types with this pallet-specific identifier.
pub mod crypto {
	use super::KEY_TYPE;
	use sp_core::sr25519::Signature as Sr25519Signature;
	use sp_runtime::{
		app_crypto::{app_crypto, sr25519},
		traits::Verify,
	};
	use sp_runtime::{MultiSignature, MultiSigner};
	app_crypto!(sr25519, KEY_TYPE);

	pub struct TestAuthId;
	// implemented for ocw-runtime
	impl frame_system::offchain::AppCrypto<MultiSigner, MultiSignature> for TestAuthId {
		type RuntimeAppPublic = Public;
		type GenericSignature = sp_core::sr25519::Signature;
		type GenericPublic = sp_core::sr25519::Public;
	}
	impl frame_system::offchain::AppCrypto<<Sr25519Signature as Verify>::Signer, Sr25519Signature> for TestAuthId {
		type RuntimeAppPublic = Public;
		type GenericSignature = sp_core::sr25519::Signature;
		type GenericPublic = sp_core::sr25519::Public;
	}
}


#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

#[frame_support::pallet]
pub mod pallet {
	use frame_support::{log, dispatch::DispatchResultWithPostInfo, pallet_prelude::*};
	use frame_system::pallet_prelude::*;
	use frame_system::Config as SystemConfig;
	use codec::{Decode, Encode};
	use sp_std::str;
	use sp_std::vec::Vec;
	use frame_support::storage::IterableStorageMap;
	use frame_system::{
		self as system,
		offchain::{
			AppCrypto, CreateSignedTransaction, SendSignedTransaction, Signer,SignedPayload,SigningTypes,
		}
	};
	use sp_runtime::{
		offchain::{http, Duration},
	};

	use cumulus_primitives_core::ParaId;
	use cumulus_pallet_xcm::{Origin as CumulusOrigin, ensure_sibling_para};
	use xcm::v0::{Xcm, Error as XcmError, SendXcm, OriginKind, MultiLocation, Junction};



	#[derive(Encode, Decode, Default, PartialEq, Eq)]
	#[cfg_attr(feature = "std", derive(Debug))]
	pub struct DataInfo {
		url: Vec<u8>,
		data: Vec<u8>,
	}

	#[pallet::config]
	pub trait Config: CreateSignedTransaction<Call<Self>> + frame_system::Config {
		/// The identifier type for an offchain worker.
		type AuthorityId: AppCrypto<Self::Public, Self::Signature>;

		type Origin: From<<Self as SystemConfig>::Origin> + Into<Result<CumulusOrigin, <Self as Config>::Origin>>;


		/// The overarching event type.
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;

		/// The overarching dispatch call type.
		type Call: From<Call<Self>> + Encode;

		type XcmSender: SendXcm;

	}

	/// Payload used by this example crate to hold price
	/// data required to submit a transaction.
	#[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug)]
	pub struct PricePayload<Public, BlockNumber> {
		block_number: BlockNumber,
		price: u32,
		public: Public,
	}

	impl<T: SigningTypes> SignedPayload<T> for PricePayload<T::Public, T::BlockNumber> {
		fn public(&self) -> T::Public {
			self.public.clone()
		}
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

	#[pallet::type_value]
	pub fn InitialDataId<T: Config>() -> u64 { 10000000u64 }

	#[pallet::storage]
	// pub type DataId<T: Config> = StorageValue<_, u64>;
	pub type DataId<T: Config> =	StorageValue<_, u64, ValueQuery, InitialDataId<T>>;

	#[pallet::storage]
	#[pallet::getter(fn requested_offchain_data)]
	// Learn more about declaring storage items:
	// https://substrate.dev/docs/en/knowledgebase/runtime/storage#declaring-storage-items
	pub type RequestedOffchainData<T: Config> = StorageMap<_, Identity, u64, DataInfo, ValueQuery>;

	#[pallet::event]
	#[pallet::metadata(T::AccountId = "AccountId")]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// Event documentation should end with an array that provides descriptive names for event
		/// parameters. [data, who]
		FetchedOffchainData(u64, T::AccountId),

		FetchedOffchainDataViaXCM(ParaId, u32, Vec<u8>),
		RequestedOffchainDataViaXCM(ParaId, u32, Vec<u8>),

		ErrorRequestingData(XcmError, ParaId, u32, Vec<u8>),
		ErrorFetchingData(XcmError, ParaId, u32, Vec<u8>),
	}

	// Errors inform users that something went wrong.
	#[pallet::error]
	pub enum Error<T> {
		/// Error names should be descriptive.
		NoneValue,
		/// Errors should have helpful documentation associated with them.
		StorageOverflow,
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {

		fn offchain_worker(block_number: T::BlockNumber) {
			// Note that having logs compiled to WASM may cause the size of the blob to increase
			// significantly. You can use `RuntimeDebug` custom derive to hide details of the types
			// in WASM. The `sp-api` crate also provides a feature `disable-logging` to disable
			// all logging and thus, remove any logging from the WASM.
			log::info!("Hello World from offchain workers!");

			// Since off-chain workers are just part of the runtime code, they have direct access
			// to the storage and other included pallets.
			//
			// We can easily import `frame_system` and retrieve a block hash of the parent block.
			let parent_hash = <system::Pallet<T>>::block_hash(block_number - 1u32.into());
			log::debug!("Current block: {:?} (parent hash: {:?})", block_number, parent_hash);

			// It's a good practice to keep `fn offchain_worker()` function minimal, and move most
			// of the code to separate `impl` block.
			// Here we call a helper function to calculate current average price.
			// This function reads storage entries of the current state.
			let res = Self::fetch_data_and_send_signed();
			if let Err(e) = res {
				log::error!("Error: {}", e);
			}
		}
	}

	// Dispatchable functions allows users to interact with the pallet and invoke state changes.
	// These functions materialize as "extrinsics", which are often compared to transactions.
	// Dispatchable functions must be annotated with a weight and must return a DispatchResult.
	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// An example dispatchable that takes a singles value as a parameter, writes the value to
		/// storage and emits an event. This function must be dispatched by a signed extrinsic.
		#[pallet::weight(10_000 + T::DbWeight::get().writes(1))]
		pub fn submit_request_data(origin: OriginFor<T>, index: u64, data: Vec<u8>) -> DispatchResultWithPostInfo {
			// Check that the extrinsic was signed and get the signer.
			// This function will return an error if the extrinsic is not signed.
			// https://substrate.dev/docs/en/knowledgebase/runtime/origin
			let who = ensure_signed(origin)?;

			let info = Self::requested_offchain_data(index);
			<RequestedOffchainData<T>>::insert(index, DataInfo {
				url: info.url,
				data: data,
			});


			// Emit an event.
			Self::deposit_event(Event::FetchedOffchainData(index, who));
			// Return a successful DispatchResultWithPostInfo
			Ok(().into())
		}

		/// An example dispatchable that may throw a custom error.
		#[pallet::weight(10_000 + T::DbWeight::get().reads_writes(1,1))]
		pub fn cause_error(origin: OriginFor<T>) -> DispatchResultWithPostInfo {
			let _who = ensure_signed(origin)?;

			// Read a value from storage.
			match <DataId<T>>::get() {
				// Return an error if the value has not been set.
				// None => Err(Error::<T>::NoneValue)?,
				old => {
					// Increment the value read from storage; will error in the event of overflow.
					let new = old.checked_add(1).ok_or(Error::<T>::StorageOverflow)?;
					// Update the value in storage with the incremented result.
					<DataId<T>>::put(new);
					Ok(().into())
				}
			}
		}

		#[pallet::weight(0)]
		pub fn request_offchain_data_cross_chain(origin: OriginFor<T>, seq: u32, data: Vec<u8>) -> DispatchResult {
			let para = ensure_sibling_para(<T as Config>::Origin::from(origin))?;
			log::info!("************** request off chain data ****************");
			log::info!("Paraid value is {:?}", para);
			log::info!("Data value is {}", str::from_utf8(&data).unwrap());
			match T::XcmSender::send_xcm(
				MultiLocation::X2(Junction::Parent, Junction::Parachain(para.into())),
				Xcm::Transact {
					origin_type: OriginKind::Native,
					require_weight_at_most: 1_000,
					call: <T as Config>::Call::from(Call::<T>::fetch_offchain_data_cross_chain(seq, data.clone())).encode().into(),
				},
			) {
				Ok(()) => Self::deposit_event(Event::RequestedOffchainDataViaXCM(para,seq, data)),
				Err(e) => Self::deposit_event(Event::ErrorRequestingData(e, para, seq, data)),
			}


			Ok(())
		}

		#[pallet::weight(0)]
		fn fetch_offchain_data_cross_chain(origin: OriginFor<T>, seq: u32, data: Vec<u8>) -> DispatchResult {
			// Only accept pings from other chains.
			let para = ensure_sibling_para(<T as Config>::Origin::from(origin))?;
			log::info!("************** fetch off chain data ****************");
			log::info!("Paraid value of caller is {:?}", para);
			log::info!("Data value passed is {}", str::from_utf8(&data).unwrap());

			let url = "https://min-api.cryptocompare.com/data/price?fsym=BTC&tsyms=USD";
			let res = Self::fetch_http_get_result(url).unwrap_or("Failed fetch data".as_bytes().to_vec());

			match T::XcmSender::send_xcm(
				MultiLocation::X2(Junction::Parent, Junction::Parachain(para.into())),
				Xcm::Transact {
					origin_type: OriginKind::Native,
					require_weight_at_most: 1_000,
					call: <T as Config>::Call::from(Call::<T>::request_offchain_data_cross_chain(seq, res.clone())).encode().into(),
				},
			) {
				Ok(()) => Self::deposit_event(Event::FetchedOffchainDataViaXCM(para,seq, data)),
				Err(e) => Self::deposit_event(Event::ErrorFetchingData(e, para, seq, data)),
			}
			Ok(())
		}


		#[pallet::weight(10_000 + T::DbWeight::get().reads_writes(1,1))]
		pub fn add_fetch_data_request(_origin: OriginFor<T>, url: Vec<u8>) -> DispatchResult {
			let index = DataId::<T>::get();
			DataId::<T>::put(index + 1u64);
			<RequestedOffchainData<T>>::insert(index, DataInfo {
				url: url,
				data: Vec::new(),
			});

			Ok(())
		}
	}


	impl<T: Config> Pallet<T> {

		/// A helper function to fetch the price and send signed transaction.
		fn fetch_data_and_send_signed() -> Result<(), &'static str> {
			let signer = Signer::<T, T::AuthorityId>::all_accounts();
			if !signer.can_sign() {
				return Err(
					"No local accounts available. Consider adding one via `author_insertKey` RPC.",
				)?;
			}
			log::info!("range RequestedOffchainData");
			for (key, val) in <RequestedOffchainData<T> as IterableStorageMap<u64, DataInfo>>::iter() {
				let url = str::from_utf8(&val.url).unwrap();
				log::info!("with RequestedOffchainData: {}, {}", key, url);
				let res = Self::fetch_http_get_result(url).unwrap_or("Failed fetch data".as_bytes().to_vec());
				log::info!("set val.data: {}", str::from_utf8(&res).unwrap());

				// Using `send_signed_transaction` associated type we create and submit a transaction
				// representing the call, we've just created.
				// Submit signed will return a vector of results for all accounts that were found in the
				// local keystore with expected `KEY_TYPE`.
				let results = signer.send_signed_transaction(|_account| Call::submit_request_data(key, res.clone()));
				for (acc, res) in &results {
					match res {
						Ok(()) => log::info!("[{:?}] Submitted data {}", acc.id, key),
						Err(e) => log::error!("[{:?}] Failed to submit transaction: {:?}", acc.id, e),
					}
				}
			}

			Ok(())
		}

		/// Fetch current price and return the result in cents.
		fn fetch_http_get_result(url: &str) -> Result<Vec<u8>, http::Error> {
			// We want to keep the offchain worker execution time reasonable, so we set a hard-coded
			// deadline to 2s to complete the external call.
			// You can also wait idefinitely for the response, however you may still get a timeout
			// coming from the host machine.
			let deadline = sp_io::offchain::timestamp().add(Duration::from_millis(2_000));
			// Initiate an external HTTP GET request.
			// This is using high-level wrappers from `sp_runtime`, for the low-level calls that
			// you can find in `sp_io`. The API is trying to be similar to `reqwest`, but
			// since we are running in a custom WASM execution environment we can't simply
			// import the library here.
			let request = http::Request::get(url);
			// We set the deadline for sending of the request, note that awaiting response can
			// have a separate deadline. Next we send the request, before that it's also possible
			// to alter request headers or stream body content in case of non-GET requests.
			let pending = request
				.deadline(deadline)
				.send()
				.map_err(|_| http::Error::IoError)?;

			// The request is already being processed by the host, we are free to do anything
			// else in the worker (we can send multiple concurrent requests too).
			// At some point however we probably want to check the response though,
			// so we can block current thread and wait for it to finish.
			// Note that since the request is being driven by the host, we don't have to wait
			// for the request to have it complete, we will just not read the response.
			let response = pending
				.try_wait(deadline)
				.map_err(|_| http::Error::DeadlineReached)??;
			// Let's check the status code before we proceed to reading the response.
			if response.code != 200 {
				log::info!("Unexpected status code: {}", response.code);
				return Err(http::Error::Unknown);
			}

			// Next we want to fully read the response body and collect it to a vector of bytes.
			// Note that the return object allows you to read the body in chunks as well
			// with a way to control the deadline.
			let body = response.body().collect::<Vec<u8>>();

			// Create a str slice from the body.
			let body_str = sp_std::str::from_utf8(&body).map_err(|_| {
				log::info!("No UTF8 body");
				http::Error::Unknown
			})?;
			log::info!("fetch_http_get_result Got {} result: {}", url, body_str);

			Ok(body_str.as_bytes().to_vec())
		}

	}
}
