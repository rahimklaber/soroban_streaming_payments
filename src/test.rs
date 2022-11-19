use soroban_auth::{Identifier, Signature};
use soroban_sdk::{Env, AccountId, BytesN, IntoVal, testutils::{Accounts, Ledger, LedgerInfo}, BigInt};

use crate::{token::{self, TokenMetadata}, StreamingContract, StreamingContractClient, Stream};

fn create_token_contract(e: &Env, admin: &AccountId) -> (BytesN<32>, token::Client) {
    let id = e.register_contract_token(None);
    let token = token::Client::new(e, &id);
    // decimals, name, symbol don't matter in tests
    token.init(
        &Identifier::Account(admin.clone()),
        &TokenMetadata {
            name: "name".into_val(e),
            symbol: "symbol".into_val(e),
            decimals: 7,
        },
    );
    (id.into(), token)
}

fn create_streaming_contract(e: &Env) -> (BytesN<32>, StreamingContractClient){
    
    let contract_id = e.register_contract(None, StreamingContract);

    let streaming_contract = StreamingContractClient::new(&e,&contract_id);

    (contract_id.into(),streaming_contract)
}

#[test]
fn test(){
    let env = Env::default();

    let user_1 = env.accounts().generate();
    let user_2 = env.accounts().generate();

    let (token_contract_id, token_client) = create_token_contract(&env, &user_1);

    let (streaming_contract_id, stream_client) = create_streaming_contract(&env);

    token_client.with_source_account(&user_1)
    .mint(&Signature::Invoker, &BigInt::from_u64(&env,0), &Identifier::Account(user_1.clone()), &BigInt::from_u64(&env,1000));

    token_client.with_source_account(&user_1)
    .approve(&Signature::Invoker, &BigInt::zero(&env), &Identifier::Contract(streaming_contract_id.clone()), &BigInt::from_u64(&env,1000));

    let stream = Stream{
        from: Identifier::Account(user_1.clone()),
        to: Identifier::Account(user_2.clone()),
        amount: BigInt::from_u64(&env,10),
        start_time: env.ledger().timestamp(),
        end_time: env.ledger().timestamp() + 10,
        tick_time: 1,
        token_c_id: token_contract_id.clone(),
        able_stop: false,
    };

    let stream_id = stream_client
    .with_source_account(&user_1)
    .c_stream(&Signature::Invoker, &BigInt::zero(&env), &stream);
    
    assert_eq!(BigInt::from_u64(&env,10),token_client.balance(&soroban_auth::Identifier::Contract(streaming_contract_id)));

    env.ledger().set(LedgerInfo {
        timestamp: env.ledger().timestamp() + 5,
        protocol_version: 1,
        sequence_number: 1,
        network_passphrase: Default::default(),
        base_reserve: 1,
    });


    stream_client.with_source_account(&user_2)
    .w_stream(&Signature::Invoker, &BigInt::zero(&env), &stream_id);

    assert_eq!(BigInt::from_u32(&env, 5),token_client.balance(&Identifier::Account(user_2.clone())));

    stream_client.with_source_account(&user_2)
    .w_stream(&Signature::Invoker, &BigInt::zero(&env), &stream_id);

    assert_eq!(BigInt::from_u32(&env, 5),token_client.balance(&Identifier::Account(user_2)));

}