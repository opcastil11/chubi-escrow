use anchor_lang::prelude::*;

#[event]
pub struct PerpMarketCreated {
    pub market_id: String,
    pub authority: Pubkey,
    pub creator: Pubkey,
}

#[event]
pub struct PerpDeposited {
    pub market_id: String,
    pub maker: Pubkey,
    pub side: u8,
    pub amount: u64,
    pub entry_weight: u64,
    pub nonce: u64,
    pub timestamp: i64,
}

#[event]
pub struct EpochCranked {
    pub market_id: String,
    pub winner_side: u8,           // side that received funding
    pub funding_lamports: u64,     // lamports moved from loser → winner
    pub imbalance_bps: u64,        // pool imbalance, in bps (10_000 = full one-sided)
    pub cranker: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct PerpExited {
    pub market_id: String,
    pub maker: Pubkey,
    pub nonce: u64,
    pub gross_value: u64,          // pre-fee fair value of the position
    pub principal: u64,            // original deposit (for profit calc)
    pub protocol_fee: u64,
    pub creator_fee: u64,
    pub net_payout: u64,
}

#[event]
pub struct PerpClosed {
    pub market_id: String,
    pub authority: Pubkey,
}

#[event]
pub struct PerpCreatorFeesClaimed {
    pub market_id: String,
    pub creator: Pubkey,
    pub amount: u64,
}

#[event]
pub struct PerpProtocolFeesCollected {
    pub market_id: String,
    pub recipient: Pubkey,
    pub amount: u64,
}
