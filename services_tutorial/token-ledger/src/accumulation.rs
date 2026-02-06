// Code support for the accumulation phase. This includes the necessary data types 
// as well as support for storage access and the actual accumulation logic.

use super::*;

/// Validated operations to apply in accumulation
#[derive(Clone, Debug, Encode, Decode)]
pub enum Operation {
    Mint {
        to: AccountId,
        token_id: TokenId,
        amount: u64,
    },
    Transfer {
        from: AccountId,
        to: AccountId,
        token_id: TokenId,
        amount: u64,
    }
}

/// Storage key for an account's token balance
/// Format: "bal:" || token_id (4 bytes LE) || account_id (32 bytes)
pub const BALANCE_KEY_SIZE: usize = 4 + 4 + 32;
pub fn balance_key(token_id: TokenId, account: &AccountId) -> [u8; BALANCE_KEY_SIZE] {
    const PREFIX_BALANCE: &[u8] = b"bal:";
    const PREFIX_LENGTH: usize = PREFIX_BALANCE.len();
    const TOKEN_LENGTH: usize = core::mem::size_of::<TokenId>();
    const ACCOUNT_LENGTH: usize = core::mem::size_of::<AccountId>();
    let mut key = [0u8; PREFIX_BALANCE.len() + TOKEN_LENGTH + ACCOUNT_LENGTH];

    key[..PREFIX_LENGTH].copy_from_slice(PREFIX_BALANCE);
    key[PREFIX_LENGTH..PREFIX_LENGTH + TOKEN_LENGTH].copy_from_slice(&token_id.to_le_bytes());
    key[PREFIX_LENGTH + TOKEN_LENGTH..].copy_from_slice(account);
    key
}

pub fn on_transfer(item: TransferRecord) {
    use crate::alloc::string::ToString;

    let TransferRecord { source, amount, memo, .. } = item;
    info!(
        "Received transfer from {source} of {amount} with memo {}",
        alloc::string::String::from_utf8(memo.to_vec()).unwrap_or("[...]".to_string())
    );
}
pub fn on_work_item(record: WorkItemRecord) {
    info!("Accumulate processing work item record: package {:?}", record.package);
    let output = match record.result {
        Ok(output) => output,
        Err(e) => {
            warn!("Work item failed: {:?}", e);
            return;
	}
    };

    let Ok(operations) = Vec::<Operation>::decode(&mut &output[..]) else {
        warn!("Failed to decode validated operations");
        return;
    };

    info!("Processing {} validated operations", operations.len());

    for op in operations {
        match op {
            Operation::Mint { to, token_id, amount } => process_mint(to, token_id, amount),
            Operation::Transfer { from, to, token_id, amount } => process_transfer(from, to, token_id, amount),
        }
    }
}


fn process_mint(to: AccountId, token_id: TokenId, amount: u64) {
    use jam_pvm_common::accumulate::{checkpoint, get, get_storage, set, set_storage};

    let mut known_tokens: Vec<TokenId> = get("known_token_ids").unwrap_or_default();

    if known_tokens.contains(&token_id) {
        warn!("Minting already minted token: {}", token_id);
        return;
    }

    known_tokens.push(token_id);
    let _ = set("known_token_ids", &known_tokens);

    let to_key = balance_key(token_id, &to);

    let current_bal: u64 =
        get_storage(&to_key).and_then(|b| u64::decode(&mut &b[..]).ok()).unwrap_or(0);

    let new_bal = current_bal.saturating_add(amount);
    let _ = set_storage(&to_key, &new_bal.encode());

    info!(
        "Minted {} of token {} to controller account {:?}. New balance: {}",
        amount,
        token_id,
    	hex::encode(to),
        new_bal
    );

    checkpoint();
}

fn process_transfer(from: AccountId, to: AccountId, token_id: TokenId, amount: u64) {
    use jam_pvm_common::accumulate::{checkpoint, get, get_storage, set_storage};

    let known_tokens: Vec<TokenId> = get("known_token_ids").unwrap_or_default();

    if !known_tokens.contains(&token_id) {
        warn!("Trying to transfer unknown token: {}", token_id);
        return;
    }

    let from_key = balance_key(token_id, &from);
    let to_key = balance_key(token_id, &to);

    let from_bal: u64 =
        get_storage(&from_key).and_then(|b| u64::decode(&mut &b[..]).ok()).unwrap_or(0);

    if from_bal < amount {
        warn!(
            "Insufficient balance: account {:?} has {} but tried to send {}",
            hex::encode(from),
            from_bal,
            amount
        );
        return;
    }

    let to_bal: u64 = get_storage(&to_key).and_then(|b| u64::decode(&mut &b[..]).ok()).unwrap_or(0);

    let _ = set_storage(&from_key, &(from_bal - amount).encode());
    let _ = set_storage(&to_key, &(to_bal.saturating_add(amount)).encode());

    info!(
        "Transferred {} of token {} from {:?} to {:?}",
        amount,
        token_id,
        hex::encode(from),
        hex::encode(to)
    );

    checkpoint();
}

