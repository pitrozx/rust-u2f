use crate::{
    authenticator::CredentialHandle,
    crypto::{AttestationSource, PrivateKeyCredentialSource, PublicKeyCredentialSource},
    CredentialStore,
};
use async_trait::async_trait;
use fido2_api::{
    Aaguid, AttestationCertificate, AttestationStatement, AttestedCredentialData,
    AuthenticatorData, PackedAttestationStatement, PublicKeyCredentialDescriptor,
    PublicKeyCredentialParameters, PublicKeyCredentialRpEntity, RelyingPartyIdentifier, Sha256,
    UserHandle,
};
use std::sync::Mutex;

pub trait CredentialStorage {
    type Error;

    fn put_discoverable(
        &mut self,
        credential: PrivateKeyCredentialSource,
    ) -> Result<(), Self::Error>;

    fn get(
        &self,
        credential_handle: &CredentialHandle,
    ) -> Result<Option<PrivateKeyCredentialSource>, Self::Error>;

    fn list_discoverable(
        &self,
        rp_id: &RelyingPartyIdentifier,
    ) -> Result<Vec<CredentialHandle>, Self::Error>;

    fn list_specified(
        &self,
        rp_id: &RelyingPartyIdentifier,
        credential_list: &[PublicKeyCredentialDescriptor],
    ) -> Result<Vec<CredentialHandle>, Self::Error>;
}

pub struct SoftwareCryptoStore<S>(Mutex<Data<S>>);

impl<S> SoftwareCryptoStore<S> {
    pub fn new(
        store: S,
        aaguid: Aaguid,
        attestation_source: AttestationSource,
        rng: ring::rand::SystemRandom,
    ) -> Self {
        Self(Mutex::new(Data {
            aaguid,
            rng,
            store,
            attestation_source,
        }))
    }
}

pub(crate) struct Data<S> {
    aaguid: Aaguid,
    rng: ring::rand::SystemRandom,
    store: S,
    attestation_source: AttestationSource,
}

#[async_trait(?Send)]
impl<S: CredentialStorage> CredentialStore for SoftwareCryptoStore<S>
where
    S: CredentialStorage,
    S::Error: From<ring::error::Unspecified>,
{
    type Error = S::Error;

    async fn make_credential(
        &self,
        parameters: &PublicKeyCredentialParameters,
        rp: &PublicKeyCredentialRpEntity,
        user_handle: &UserHandle,
    ) -> Result<CredentialHandle, Self::Error> {
        let mut this = self.0.lock().unwrap();
        let key = PrivateKeyCredentialSource::generate(parameters, rp, user_handle, &this.rng)?;
        let handle = key.handle();
        this.store.put_discoverable(key)?;
        Ok(handle)
    }

    async fn attest(
        &self,
        rp_id: &fido2_api::RelyingPartyIdentifier,
        credential_handle: &CredentialHandle,
        client_data_hash: &fido2_api::Sha256,
        user_present: bool,
        user_verified: bool,
    ) -> Result<(AuthenticatorData, AttestationStatement), Self::Error> {
        let this = self.0.lock().unwrap();
        if let Some(key) = this.store.get(credential_handle)? {
            let key: PublicKeyCredentialSource = key.try_into().unwrap();
            let auth_data = AuthenticatorData {
                rp_id_hash: Sha256::digest(rp_id.as_bytes()),
                user_present,
                user_verified,
                sign_count: 1, // TODO increment use counter
                attested_credential_data: Some(vec![AttestedCredentialData {
                    aaguid: this.aaguid,
                    credential_id: credential_handle.descriptor.id.clone(),
                    credential_public_key: key.credential_public_key(),
                }]),
            };
            let signature = this
                .attestation_source
                .sign(&auth_data, client_data_hash, &this.rng)
                .unwrap();
            Ok((
                auth_data,
                AttestationStatement::Packed(PackedAttestationStatement {
                    alg: key.alg(),
                    sig: signature,
                    x5c: Some(AttestationCertificate {
                        attestation_certificate: this
                            .attestation_source
                            .public_key_document()
                            .as_ref()
                            .to_vec(),
                        ca_certificate_chain: vec![],
                    }),
                }),
            ))
        } else {
            todo!("error")
        }
    }

    async fn assert(
        &self,
        rp_id: &RelyingPartyIdentifier,
        credential_handle: &CredentialHandle,
        client_data_hash: &Sha256,
        user_present: bool,
        user_verified: bool,
    ) -> Result<(AuthenticatorData, fido2_api::Signature), Self::Error> {
        let this = self.0.lock().unwrap();
        if let Some(key) = this.store.get(credential_handle)? {
            let key: PublicKeyCredentialSource = key.try_into().unwrap();
            let auth_data = AuthenticatorData {
                rp_id_hash: Sha256::digest(rp_id.as_bytes()),
                user_present,
                user_verified,
                sign_count: 2,
                attested_credential_data: None,
            };
            // TODO increment use counter
            let signature = key.sign(&auth_data, client_data_hash, &this.rng).unwrap();
            Ok((auth_data, signature))
        } else {
            todo!("error")
        }
    }

    async fn list_discoverable_credentials(
        &self,
        rp_id: &RelyingPartyIdentifier,
    ) -> Result<Vec<CredentialHandle>, Self::Error> {
        let this = self.0.lock().unwrap();
        this.store.list_discoverable(rp_id)
    }

    async fn list_specified_credentials(
        &self,
        rp_id: &RelyingPartyIdentifier,
        credential_list: &[PublicKeyCredentialDescriptor],
    ) -> Result<Vec<CredentialHandle>, Self::Error> {
        let this = self.0.lock().unwrap();
        this.store.list_specified(rp_id, credential_list)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn pass() {}
}
