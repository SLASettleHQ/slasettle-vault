use soroban_sdk::{contractevent, Address};

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SlaCreated {
    #[topic]
    pub sla_id: u64,
    #[topic]
    pub provider: Address,
    pub token: Address,
    pub bond_amount: i128,
    pub beneficiary: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BondToppedUp {
    #[topic]
    pub sla_id: u64,
    pub amount: i128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettlementPaid {
    #[topic]
    pub sla_id: u64,
    #[topic]
    pub round_id: u64,
    pub payout: i128,
    pub beneficiary: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SlaCancelled {
    #[topic]
    pub sla_id: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BondWithdrawn {
    #[topic]
    pub sla_id: u64,
    pub amount: i128,
}
