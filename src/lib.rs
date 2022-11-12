#![no_std]

use soroban_auth::{Signature, Identifier, verify};
use soroban_sdk::{contracttype, Env, BigInt, BytesN, contractimpl, contracterror, panic_error, symbol};

mod token {
    soroban_sdk::contractimport!(file = "./soroban_token_spec.wasm");
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    StreamNotExist = 1,
    NotAuthorized = 2,
    IncorrectNonceForInvoker = 3,
    IncorrectNonce = 4,
    StreamCancelled = 5,
    StreamNotCancellable = 6,
    StreamDone = 7,
}

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Stream(u64),
    StreamId,
    // extra data relating to withdrawing from the stream
    StreamData(u64),
    Nonce(Identifier)
}

#[contracttype]
#[derive(Clone,Debug)]
pub struct StreamData{
    // how much has been withdrawn
    pub a_withdraw: BigInt,
    // wether the stream was cancelled
    pub cancelled: bool
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Stream {
    pub from: Identifier,
    pub to: Identifier,
    pub amount : BigInt,
    pub start_time: u64,
    pub end_time: u64,
    // every `tick_time` there is a new tick
    pub tick_time: u64,
    // token contract id
    pub token_c_id : BytesN<32>,
    //whether the creator can cancell the stream.
    pub able_stop : bool
}



pub trait StreamingTrait {
    //create stream
    fn c_stream(env: Env, signature: Signature, nonce: BigInt, stream : Stream) -> u64;
    // withdraw from streaam
    fn w_stream(env: Env, signature: Signature, nonce: BigInt, stream_id : u64);
    //cancell/stop stream
    fn s_stream(env: Env, signature: Signature, stream_id : u64);

    fn get_stream(env: Env, stream_id : u64) -> (Stream,StreamData);
    fn nonce(env: Env, id: Identifier) -> BigInt;
}

pub struct  StreamingContract;

#[contractimpl]
impl StreamingTrait for StreamingContract{
    // create the stream by sending withdrawable funds to this contract
    // returns the id of the created stream
    fn c_stream(env: Env, signature: Signature, nonce: BigInt, stream : Stream) -> u64 {
        let id = signature.identifier(&env);

        // check that the signature is valid
        verify(&env, &signature, symbol!("c_stream"), (&id, &nonce));

        //consume and check that nonce is valid
        verify_and_consume_nonce(&env, &signature, &nonce);

        token::Client::new(&env, stream.token_c_id.clone())
        .xfer_from(&soroban_auth::Signature::Invoker, &BigInt::from_u32(&env, 0),&stream.from ,&soroban_auth::Identifier::Contract(env.current_contract()), &stream.amount);

        let stream_id = get_and_inc_stream_id(&env);

        // store stream
        env.data()
        .set(DataKey::Stream(stream_id),stream);

        // store mutable stream data
        env.data()
        .set(DataKey::StreamData(stream_id), StreamData{
            a_withdraw: BigInt::zero(&env),
            cancelled: false 
        });

        //return stream id
        stream_id
    }
    // withdraw from stream
    fn w_stream(env: Env, signature: Signature, nonce: BigInt, stream_id: u64){
        let stream = get_stream(&env, stream_id);
        let stream_data = get_stream_data(&env, stream_id);

        let id = signature.identifier(&env);

        //check if user is the recipient of the stream
        if id != stream.to{
            panic_error!(&env, Error::NotAuthorized);
        }

        // check if stream has been cancelled
        if stream_data.cancelled{
            panic_error!(&env, Error::StreamCancelled);
        }

        // check if all tokens have been withdrawn
        if stream_data.a_withdraw == stream.amount{
            panic_error!(&env, Error::StreamDone);
        }

        // check that the signature is valid
        verify(&env, &signature, symbol!("w_stream"), (&id, &nonce));

        //consume and check that nonce is valid
        verify_and_consume_nonce(&env, &signature, &nonce);


        // if we are over the end of the stream, then withdraw everything.
        if stream.end_time < env.ledger().timestamp(){
            token::Client::new(&env, stream.token_c_id.clone())
                .xfer(&Signature::Invoker, &BigInt::zero(&env), &stream.to, &(&stream.amount - &stream_data.a_withdraw));

            update_amount_withdrawn(&env, stream_id, stream.amount);
            return
        }

        // stream duration
        let duration = stream.end_time - stream.start_time;

        let mut total_ticks = duration / stream.tick_time;
        // round up the total ticks
        if duration % stream.tick_time != 0{
            total_ticks += 1;
        }
        let amount_per_tick = stream.amount / total_ticks;

        let time_elapsed = env.ledger().timestamp() - stream.start_time;
        // elsapsed ticks
        let elapsed_ticks = time_elapsed / stream.tick_time;

        // get the amount of funds that we can withdraw minus the amount we have allready withdrawn
        let amount_to_withdraw = amount_per_tick * elapsed_ticks - &stream_data.a_withdraw;

        token::Client::new(&env, stream.token_c_id.clone())
        .xfer(&Signature::Invoker, &BigInt::zero(&env), &stream.to, &amount_to_withdraw);

        update_amount_withdrawn(&env, stream_id, &stream_data.a_withdraw + &amount_to_withdraw);
    }
    //stop stream if it is cancellable and return the available funds back to the creataor of the stream
    fn s_stream(env: Env, signature: Signature, stream_id: u64){
        let stream = get_stream(&env, stream_id);
        let stream_data = get_stream_data(&env, stream_id);

        let id = signature.identifier(&env);

        // check if creator of stream
        if stream.from != id{
            panic_error!(&env, Error::NotAuthorized);
        } 

        // check if stream is cancellable
        if !stream.able_stop{
            panic_error!(&env, Error::StreamNotCancellable);
        }
        // check if stream is allready cancelled
        if stream_data.cancelled{
            panic_error!(&env, Error::StreamCancelled);
        }
        // dont need nonce, since we can only stop once.
        verify(&env, &signature, symbol!("s_stream"), (&id, stream_id));

        // send back everything that wasn't withdrawn
        token::Client::new(&env, stream.token_c_id.clone())
                .xfer(&Signature::Invoker, &BigInt::zero(&env), &id, &(&stream.amount - &stream_data.a_withdraw));

        set_stream_data_cancelled(&env, stream_id);
    }
    // retrieve stream and additional stream data
    fn get_stream(env: Env, stream_id: u64) -> (Stream,StreamData){
        (get_stream(&env, stream_id), get_stream_data(&env, stream_id))
    }

    fn nonce(env: Env, id: Identifier) -> BigInt {
        get_nonce(&env, &id)
    }
}
fn get_and_inc_stream_id(env: &Env) -> u64 {
    let prev = env
        .data()
        .get(DataKey::StreamId)
        .unwrap_or(Ok(0u64))
        .unwrap();

    env.data().set(DataKey::StreamId, prev + 1);
    prev
}

fn get_stream(env: &Env, stream_id: u64) -> Stream{
    let data: Option<Result<Stream, _>> = env.data()
        .get(DataKey::Stream(stream_id));
    
    match data{
        Some(Ok(stream)) => stream,
        _ => panic_error!(&env,Error::StreamNotExist),
    }
}

fn get_stream_data(env: &Env, stream_id: u64) -> StreamData{
    let data: Option<Result<StreamData, _>> = env.data()
    .get(DataKey::StreamData(stream_id));

    match data{
        Some(Ok(stream)) => stream,
        _ => panic_error!(&env,Error::StreamNotExist),
    }
}

fn set_stream_data_cancelled(env: &Env, stream_id: u64){
    env.data()
    .set(DataKey::StreamData(stream_id), StreamData{
        a_withdraw: BigInt::zero(env), //not sure if this should be the value withdrawn by the recipient. Technically, its not needed anymore, but it might be usefull.
        cancelled: true
    })
}

fn update_amount_withdrawn(env: &Env, stream_id: u64, total_amount_withdrawn: BigInt){
    env.data()
    .set(DataKey::StreamData(stream_id),total_amount_withdrawn);
}

fn verify_and_consume_nonce(env: &Env, sig: &Signature, nonce: &BigInt) {
    match sig {
        Signature::Invoker => {
            if BigInt::zero(env) != nonce {
                panic_error!(env, Error::IncorrectNonceForInvoker);
            }
        }
        Signature::Ed25519(_) | Signature::Account(_) => {
            let id = sig.identifier(env);
            if nonce != &get_nonce(env, &id) {
                panic_error!(env, Error::IncorrectNonce);
            }
            set_nonce(env, &id, nonce + 1);
        }
    }
}

fn get_nonce(env: &Env, id: &Identifier) -> BigInt {
    let key = DataKey::Nonce(id.clone());
    env.data()
        .get(key)
        .unwrap_or_else(|| Ok(BigInt::zero(env)))
        .unwrap()
}

fn set_nonce(env: &Env, id: &Identifier, nonce: BigInt) {
    let key = DataKey::Nonce(id.clone());
    env.data().set(key, nonce);
}


#[cfg(test)]
mod test;