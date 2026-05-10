use anchor_lang::prelude::*;

#[error_code]
pub enum PerpError {
    #[msg("Market ID exceeds 64 characters")]
    MarketIdTooLong,                // 6000
    #[msg("Signer wallet is not allowed to create perpetual markets")]
    NotPerpetualAdmin,              // 6001
    #[msg("Invalid side index")]
    InvalidSide,                    // 6002
    #[msg("Deposit below minimum (0.001 SOL)")]
    DepositTooSmall,                // 6003
    #[msg("Position does not belong to this market")]
    PositionMarketMismatch,         // 6004
    #[msg("Not your position")]
    NotYourPosition,                // 6005
    #[msg("Position already exited")]
    AlreadyExited,                  // 6006
    #[msg("Math overflow")]
    MathOverflow,                   // 6007
    #[msg("Insufficient vault balance")]
    InsufficientVault,              // 6008
    #[msg("Epoch has not elapsed yet")]
    EpochNotElapsed,                // 6009
    #[msg("No funding to apply (single-sided or perfectly balanced market)")]
    NoFundingNeeded,                // 6010
    #[msg("Market is closed (admin force-closed)")]
    MarketClosed,                   // 6011
    #[msg("Creator account does not match this market")]
    CreatorMarketMismatch,          // 6012
    #[msg("Signer is not the creator of this market")]
    NotCreator,                     // 6013
    #[msg("No creator fees available to claim")]
    NoCreatorFees,                  // 6014
}
