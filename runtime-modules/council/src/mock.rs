#![cfg(test)]

/////////////////// Configuration //////////////////////////////////////////////
use crate::{
    AnnouncementPeriodNr, Balance, Budget, CandidateOf, Candidates, CouncilMemberOf,
    CouncilMembers, CouncilStage, CouncilStageAnnouncing, CouncilStageElection, CouncilStageUpdate,
    CouncilStageUpdateOf, Error, GenesisConfig, Module, NextBudgetRefill, RawEvent,
    ReferendumConnection, Stage, Trait,
};

use balances;
use frame_support::traits::{Currency, Get, LockIdentifier, OnFinalize};
use frame_support::{
    impl_outer_event, impl_outer_origin, parameter_types, StorageMap, StorageValue,
};
use frame_system::{EnsureOneOf, EnsureRoot, EnsureSigned, RawOrigin};
use rand::Rng;
use referendum::{
    Balance as BalanceReferendum, CastVote, OptionResult, ReferendumManager, ReferendumStage,
    ReferendumStageRevealing,
};
use sp_core::H256;
use sp_io;
use sp_runtime::traits::Hash;
use sp_runtime::{
    testing::Header,
    traits::{BlakeTwo256, IdentityLookup},
    Perbill,
};
use staking_handler::{LockComparator, StakingManager};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::marker::PhantomData;

pub const USER_REGULAR_POWER_VOTES: u64 = 0;

pub const POWER_VOTE_STRENGTH: u64 = 10;

// uncomment this when this is moved back here from staking_handler.rs temporary file
pub const VOTER_BASE_ID: u64 = 4000;
pub const CANDIDATE_BASE_ID: u64 = VOTER_BASE_ID + VOTER_CANDIDATE_OFFSET;
pub const VOTER_CANDIDATE_OFFSET: u64 = 1000;

pub const INVALID_USER_MEMBER: u64 = 9999;

// multiplies topup value so that candidate/voter can candidate/vote multiple times
pub const TOPUP_MULTIPLIER: u64 = 10;

/////////////////// Runtime and Instances //////////////////////////////////////
// Workaround for https://github.com/rust-lang/rust/issues/26925 . Remove when sorted.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Runtime;

parameter_types! {
    pub const MinNumberOfExtraCandidates: u64 = 1;
    pub const AnnouncingPeriodDuration: u64 = 15;
    pub const IdlePeriodDuration: u64 = 27;
    pub const CouncilSize: u64 = 3;
    pub const MinCandidateStake: u64 = 11000;
    pub const CandidacyLockId: LockIdentifier = *b"council1";
    pub const CouncilorLockId: LockIdentifier = *b"council2";
    pub const ElectedMemberRewardPerBlock: u64 = 100;
    pub const ElectedMemberRewardPeriod: u64 = 10;
    pub const BudgetRefillAmount: u64 = 1000;
    // intentionally high number that prevents side-effecting tests other than  budget refill tests
    pub const BudgetRefillPeriod: u64 = 1000;
}

impl Trait for Runtime {
    type Event = TestEvent;

    type Referendum = referendum::Module<Runtime, ReferendumInstance>;

    type MembershipId = u64;
    type MinNumberOfExtraCandidates = MinNumberOfExtraCandidates;
    type CouncilSize = CouncilSize;
    type AnnouncingPeriodDuration = AnnouncingPeriodDuration;
    type IdlePeriodDuration = IdlePeriodDuration;
    type MinCandidateStake = MinCandidateStake;

    type CandidacyLock = StakingManager<Self, CandidacyLockId>;
    type CouncilorLock = StakingManager<Self, CouncilorLockId>;

    type ElectedMemberRewardPerBlock = ElectedMemberRewardPerBlock;
    type ElectedMemberRewardPeriod = ElectedMemberRewardPeriod;

    type BudgetRefillAmount = BudgetRefillAmount;
    type BudgetRefillPeriod = BudgetRefillPeriod;

    fn is_council_member_account(
        membership_id: &Self::MembershipId,
        account_id: &<Self as frame_system::Trait>::AccountId,
    ) -> bool {
        membership_id == account_id
    }
}

/////////////////// Module implementation //////////////////////////////////////

impl_outer_origin! {
    pub enum Origin for Runtime {}
}

mod event_mod {
    pub use crate::Event;
}

mod referendum_mod {
    pub use referendum::Event;
    pub use referendum::Instance0;
}

mod membership_mod {
    pub use membership::Event;
}

impl_outer_event! {
    pub enum TestEvent for Runtime {
        event_mod<T>,
        frame_system<T>,
        referendum_mod Instance0 <T>,
        balances_mod<T>,
        membership_mod<T>,
    }
}

parameter_types! {
    pub const BlockHashCount: u64 = 250;
    pub const MaximumBlockWeight: u32 = 1024;
    pub const MaximumBlockLength: u32 = 2 * 1024;
    pub const AvailableBlockRatio: Perbill = Perbill::one();
}

impl frame_system::Trait for Runtime {
    type BaseCallFilter = ();
    type Origin = Origin;
    type Index = u64;
    type BlockNumber = u64;
    type Call = ();
    type Hash = H256;
    type Hashing = BlakeTwo256;
    type AccountId = u64;
    type Lookup = IdentityLookup<Self::AccountId>;
    type Header = Header;
    type Event = TestEvent;
    type BlockHashCount = BlockHashCount;
    type MaximumBlockWeight = MaximumBlockWeight;
    type DbWeight = ();
    type BlockExecutionWeight = ();
    type ExtrinsicBaseWeight = ();
    type MaximumExtrinsicWeight = ();
    type MaximumBlockLength = MaximumBlockLength;
    type AvailableBlockRatio = AvailableBlockRatio;
    type Version = ();
    type PalletInfo = ();
    type AccountData = balances::AccountData<u64>;
    type OnNewAccount = ();
    type OnKilledAccount = ();
    type SystemWeightInfo = ();
}

/////////////////// Election module ////////////////////////////////////////////

pub type ReferendumInstance = referendum::Instance0;

thread_local! {
    // global switch for stake locking features; use it to simulate lock fails
    pub static IS_UNSTAKE_ENABLED: RefCell<(bool, )> = RefCell::new((true, ));

    // global switch used to test is_valid_option_id()
    pub static IS_OPTION_ID_VALID: RefCell<(bool, )> = RefCell::new((true, ));
}

parameter_types! {
    pub const VoteStageDuration: u64 = 19;
    pub const RevealStageDuration: u64 = 23;
    pub const MinimumVotingStake: u64 = 10000;
    pub const MaxSaltLength: u64 = 32; // use some multiple of 8 for ez testing
    pub const VotingLockId: LockIdentifier = *b"referend";
    pub const MembershipFee: u64 = 100;
    pub const MinimumPeriod: u64 = 5;
}

mod balances_mod {
    pub use balances::Event;
}

impl referendum::Trait<ReferendumInstance> for Runtime {
    type Event = TestEvent;

    type MaxSaltLength = MaxSaltLength;

    type Currency = balances::Module<Self>;
    type LockId = VotingLockId;

    type ManagerOrigin =
        EnsureOneOf<Self::AccountId, EnsureSigned<Self::AccountId>, EnsureRoot<Self::AccountId>>;

    type VotePower = u64;

    type VoteStageDuration = VoteStageDuration;
    type RevealStageDuration = RevealStageDuration;

    type MinimumStake = MinimumVotingStake;

    fn calculate_vote_power(
        account_id: &<Self as frame_system::Trait>::AccountId,
        stake: &BalanceReferendum<Self, ReferendumInstance>,
    ) -> Self::VotePower {
        let stake: u64 = u64::from(*stake);
        if *account_id == USER_REGULAR_POWER_VOTES {
            return stake * POWER_VOTE_STRENGTH;
        }

        stake
    }

    fn can_unlock_vote_stake(
        vote: &CastVote<Self::Hash, BalanceReferendum<Self, ReferendumInstance>>,
    ) -> bool {
        // trigger fail when requested to do so
        if !IS_UNSTAKE_ENABLED.with(|value| value.borrow().0) {
            return false;
        }

        <Module<Runtime> as ReferendumConnection<Runtime>>::can_unlock_vote_stake(vote).is_ok()
    }

    fn process_results(winners: &[OptionResult<Self::VotePower>]) {
        let tmp_winners: Vec<OptionResult<Self::VotePower>> = winners
            .iter()
            .map(|item| OptionResult {
                option_id: item.option_id,
                vote_power: item.vote_power.into(),
            })
            .collect();
        <Module<Runtime> as ReferendumConnection<Runtime>>::recieve_referendum_results(
            tmp_winners.as_slice(),
        );
    }

    fn is_valid_option_id(option_index: &u64) -> bool {
        if !IS_OPTION_ID_VALID.with(|value| value.borrow().0) {
            return false;
        }

        <Module<Runtime> as ReferendumConnection<Runtime>>::is_valid_candidate_id(option_index)
    }

    fn get_option_power(option_id: &u64) -> Self::VotePower {
        <Module<Runtime> as ReferendumConnection<Runtime>>::get_option_power(option_id)
    }

    fn increase_option_power(option_id: &u64, amount: &Self::VotePower) {
        <Module<Runtime> as ReferendumConnection<Runtime>>::increase_option_power(
            option_id, amount,
        );
    }
}

impl balances::Trait for Runtime {
    type Balance = u64;
    type Event = TestEvent;
    type DustRemoval = ();
    type ExistentialDeposit = ExistentialDeposit;
    type AccountStore = frame_system::Module<Self>;
    type WeightInfo = ();
    type MaxLocks = MaxLocks;
}

impl membership::Trait for Runtime {
    type Event = TestEvent;
    type MemberId = u64;
    type ActorId = u64;
    type MembershipFee = MembershipFee;
}

impl pallet_timestamp::Trait for Runtime {
    type Moment = u64;
    type OnTimestampSet = ();
    type MinimumPeriod = MinimumPeriod;
    type WeightInfo = ();
}

impl Runtime {
    pub fn _feature_option_id_valid(is_valid: bool) -> () {
        IS_OPTION_ID_VALID.with(|value| {
            *value.borrow_mut() = (is_valid,);
        });
    }
}

parameter_types! {
    pub const ExistentialDeposit: u64 = 0;
    pub const MaxLocks: u32 = 50;
}

impl LockComparator<<Runtime as balances::Trait>::Balance> for Runtime {
    fn are_locks_conflicting(
        _new_lock: &LockIdentifier,
        _existing_locks: &[LockIdentifier],
    ) -> bool {
        false
    }
}

/////////////////// Data structures ////////////////////////////////////////////

#[allow(dead_code)]
#[derive(Clone)]
pub enum OriginType<AccountId> {
    Signed(AccountId),
    //Inherent, <== did not find how to make such an origin yet
    Root,
}

#[derive(Clone)]
pub struct CandidateInfo<T: Trait> {
    pub origin: OriginType<T::AccountId>,
    pub account_id: T::MembershipId,
    pub membership_id: T::MembershipId,
    pub candidate: CandidateOf<T>,
}

#[derive(Clone)]
pub struct VoterInfo<T: Trait> {
    pub origin: OriginType<T::AccountId>,
    pub account_id: T::AccountId,
    pub commitment: T::Hash,
    pub salt: Vec<u8>,
    pub vote_for: u64,
    pub stake: Balance<T>,
}

#[derive(Clone)]
pub struct CouncilSettings<T: Trait> {
    pub council_size: u64,
    pub min_candidate_count: u64,
    pub min_candidate_stake: Balance<T>,
    pub announcing_stage_duration: T::BlockNumber,
    pub voting_stage_duration: T::BlockNumber,
    pub reveal_stage_duration: T::BlockNumber,
    pub idle_stage_duration: T::BlockNumber,
    pub election_duration: T::BlockNumber,
    pub cycle_duration: T::BlockNumber,
    pub budget_refill_amount: Balance<T>,
    pub budget_refill_period: T::BlockNumber,
}

impl<T: Trait> CouncilSettings<T>
where
    T::BlockNumber: From<u64>,
{
    pub fn extract_settings() -> CouncilSettings<T> {
        let council_size = T::CouncilSize::get();

        let reveal_stage_duration =
            <Runtime as referendum::Trait<ReferendumInstance>>::RevealStageDuration::get().into();
        let announcing_stage_duration = <T as Trait>::AnnouncingPeriodDuration::get();
        let voting_stage_duration =
            <Runtime as referendum::Trait<ReferendumInstance>>::VoteStageDuration::get().into();
        let idle_stage_duration = <T as Trait>::IdlePeriodDuration::get();

        CouncilSettings {
            council_size,
            min_candidate_count: council_size + <T as Trait>::MinNumberOfExtraCandidates::get(),
            min_candidate_stake: T::MinCandidateStake::get(),
            announcing_stage_duration,
            voting_stage_duration,
            reveal_stage_duration,
            idle_stage_duration: <T as Trait>::IdlePeriodDuration::get(),

            election_duration: reveal_stage_duration
                + announcing_stage_duration
                + voting_stage_duration,
            cycle_duration: reveal_stage_duration
                + announcing_stage_duration
                + voting_stage_duration
                + idle_stage_duration,

            budget_refill_amount: <T as Trait>::BudgetRefillAmount::get(),
            budget_refill_period: <T as Trait>::BudgetRefillPeriod::get(),
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum CouncilCycleInterrupt {
    BeforeCandidatesAnnounce,
    AfterCandidatesAnnounce,
    BeforeVoting,
    AfterVoting,
    BeforeRevealing,
    AfterRevealing,
}

#[derive(Clone)]
pub struct CouncilCycleParams<T: Trait> {
    pub council_settings: CouncilSettings<T>,
    pub cycle_start_block_number: T::BlockNumber,

    // council members
    pub expected_initial_council_members: Vec<CouncilMemberOf<T>>,

    // council members after cycle finishes
    pub expected_final_council_members: Vec<CouncilMemberOf<T>>,

    // candidates announcing their candidacy
    pub candidates_announcing: Vec<CandidateInfo<T>>,

    // expected list of candidates after announcement period is over
    pub expected_candidates: Vec<CandidateOf<T>>,

    // voters that will participate in council voting
    pub voters: Vec<VoterInfo<T>>,

    // info about when should be cycle interrupted (used to customize the test)
    pub interrupt_point: Option<CouncilCycleInterrupt>,
}

/////////////////// Util macros ////////////////////////////////////////////////
macro_rules! escape_checkpoint {
    ($item:expr, $expected_value:expr) => {
        if $item == $expected_value {
            return;
        }
    };
    ($item:expr, $expected_value:expr, $return_value:expr) => {
        if $item == $expected_value {
            return $c;
        }
    };
}

/////////////////// Utility mocks //////////////////////////////////////////////

pub fn default_genesis_config() -> GenesisConfig<Runtime> {
    GenesisConfig::<Runtime> {
        stage: CouncilStageUpdate::default(),
        council_members: vec![],
        candidates: vec![],
        announcement_period_nr: 0,
        budget: 0,
        next_reward_payments: 0,
        next_budget_refill: <Runtime as Trait>::BudgetRefillPeriod::get(),
    }
}

pub fn build_test_externalities(config: GenesisConfig<Runtime>) -> sp_io::TestExternalities {
    let mut t = frame_system::GenesisConfig::default()
        .build_storage::<Runtime>()
        .unwrap();

    config.assimilate_storage(&mut t).unwrap();

    let mut result = Into::<sp_io::TestExternalities>::into(t.clone());

    // Make sure we are not in block 1 where no events are emitted
    // see https://substrate.dev/recipes/2-appetizers/4-events.html#emitting-events
    result.execute_with(|| InstanceMockUtils::<Runtime>::increase_block_number(1));

    result
}

pub struct InstanceMockUtils<T: Trait> {
    _dummy: PhantomData<T>, // 0-sized data meant only to bound generic parameters
}

impl<T: Trait> InstanceMockUtils<T>
where
    T::AccountId: From<u64>,
    T::MembershipId: From<u64>,
    T::BlockNumber: From<u64> + Into<u64>,
    Balance<T>: From<u64> + Into<u64>,
{
    pub fn mock_origin(origin: OriginType<T::AccountId>) -> T::Origin {
        match origin {
            OriginType::Signed(account_id) => T::Origin::from(RawOrigin::Signed(account_id)),
            OriginType::Root => RawOrigin::Root.into(),
            //_ => panic!("not implemented"),
        }
    }

    pub fn increase_block_number(increase: u64) -> () {
        let block_number = frame_system::Module::<T>::block_number();

        for i in 0..increase {
            let tmp_index: T::BlockNumber = block_number + i.into();

            <Module<T> as OnFinalize<T::BlockNumber>>::on_finalize(tmp_index);
            <referendum::Module<Runtime, ReferendumInstance> as OnFinalize<
                <Runtime as frame_system::Trait>::BlockNumber,
            >>::on_finalize(tmp_index.into());

            frame_system::Module::<T>::set_block_number(tmp_index + 1.into());
        }
    }

    // topup currency to the account
    fn topup_account(account_id: u64, amount: Balance<T>) {
        let _ = balances::Module::<Runtime>::deposit_creating(&account_id, amount.into());
    }

    pub fn generate_candidate(index: u64, stake: Balance<T>) -> CandidateInfo<T> {
        let account_id = CANDIDATE_BASE_ID + index;
        let origin = OriginType::Signed(account_id.into());
        let candidate = CandidateOf::<T> {
            staking_account_id: account_id.into(),
            reward_account_id: account_id.into(),
            cycle_id: AnnouncementPeriodNr::get(),
            stake,
            vote_power: 0.into(),
            note_hash: None,
        };

        Self::topup_account(account_id.into(), stake * TOPUP_MULTIPLIER.into());

        CandidateInfo {
            origin,
            candidate,
            membership_id: account_id.into(),
            account_id: account_id.into(),
        }
    }

    pub fn generate_voter(
        index: u64,
        stake: Balance<T>,
        vote_for_index: u64,
        cycle_id: u64,
    ) -> VoterInfo<T> {
        let account_id = VOTER_BASE_ID + index;
        let origin = OriginType::Signed(account_id.into());
        let (commitment, salt) =
            Self::vote_commitment(&account_id.into(), &vote_for_index.into(), &cycle_id);

        Self::topup_account(account_id.into(), stake);

        VoterInfo {
            origin,
            account_id: account_id.into(),
            commitment,
            salt,
            vote_for: vote_for_index,
            stake,
        }
    }

    pub fn generate_salt() -> Vec<u8> {
        let mut rng = rand::thread_rng();

        rng.gen::<u64>().to_be_bytes().to_vec()
    }

    pub fn vote_commitment(
        account_id: &<T as frame_system::Trait>::AccountId,
        vote_option_index: &u64,
        cycle_id: &u64,
    ) -> (T::Hash, Vec<u8>) {
        let salt = Self::generate_salt();

        (
            T::Referendum::calculate_commitment(account_id, &salt, &cycle_id, vote_option_index),
            salt.to_vec(),
        )
    }
}

/////////////////// Mocks of Module's actions //////////////////////////////////

pub struct InstanceMocks<T: Trait> {
    _dummy: PhantomData<T>, // 0-sized data meant only to bound generic parameters
}

impl<T: Trait> InstanceMocks<T>
where
    T::AccountId: From<u64> + Into<u64>,
    T::MembershipId: From<u64>,
    T::BlockNumber: From<u64> + Into<u64>,
    Balance<T>: From<u64> + Into<u64>,

    T::Hash:
        From<<Runtime as frame_system::Trait>::Hash> + Into<<Runtime as frame_system::Trait>::Hash>,
    T::Origin: From<<Runtime as frame_system::Trait>::Origin>
        + Into<<Runtime as frame_system::Trait>::Origin>,
    <T::Referendum as ReferendumManager<T::Origin, T::AccountId, T::Hash>>::VotePower:
        From<u64> + Into<u64>,
    T::MembershipId: Into<T::AccountId>,
{
    pub fn check_announcing_period(
        expected_update_block_number: T::BlockNumber,
        expected_state: CouncilStageAnnouncing,
    ) -> () {
        // check stage is in proper state
        assert_eq!(
            Stage::<T>::get(),
            CouncilStageUpdateOf::<T> {
                stage: CouncilStage::Announcing(expected_state),
                changed_at: expected_update_block_number,
            },
        );
    }

    pub fn check_election_period(
        expected_update_block_number: T::BlockNumber,
        expected_state: CouncilStageElection,
    ) -> () {
        // check stage is in proper state
        assert_eq!(
            Stage::<T>::get(),
            CouncilStageUpdateOf::<T> {
                stage: CouncilStage::Election(expected_state),
                changed_at: expected_update_block_number,
            },
        );
    }

    pub fn check_idle_period(expected_update_block_number: T::BlockNumber) -> () {
        // check stage is in proper state
        assert_eq!(
            Stage::<T>::get(),
            CouncilStageUpdateOf::<T> {
                stage: CouncilStage::Idle,
                changed_at: expected_update_block_number,
            },
        );
    }

    pub fn check_council_members(expect_members: Vec<CouncilMemberOf<T>>) -> () {
        // check stage is in proper state
        assert_eq!(CouncilMembers::<T>::get(), expect_members,);
    }

    pub fn check_referendum_revealing(
        //        candidate_count: u64,
        winning_target_count: u64,
        intermediate_winners: Vec<
            OptionResult<
                <T::Referendum as ReferendumManager<T::Origin, T::AccountId, T::Hash>>::VotePower,
            >,
        >,
        intermediate_results: BTreeMap<
            u64,
            <T::Referendum as ReferendumManager<T::Origin, T::AccountId, T::Hash>>::VotePower,
        >,
        expected_update_block_number: T::BlockNumber,
    ) {
        // check stage is in proper state
        assert_eq!(
            referendum::Stage::<Runtime, ReferendumInstance>::get(),
            ReferendumStage::Revealing(ReferendumStageRevealing {
                winning_target_count,
                started: expected_update_block_number.into(),
                intermediate_winners: intermediate_winners
                    .iter()
                    .map(|item| OptionResult {
                        option_id: item.option_id,
                        vote_power: item.vote_power.into(),
                    })
                    .collect(),
                current_cycle_id: AnnouncementPeriodNr::get(),
            }),
        );

        // check intermediate results
        for (key, value) in intermediate_results {
            let membership_id: T::MembershipId = key.into();

            assert!(Candidates::<T>::contains_key(membership_id));
            assert_eq!(Candidates::<T>::get(membership_id).vote_power, value);
        }
    }

    pub fn check_announcing_stake(membership_id: &T::MembershipId, amount: Balance<T>) {
        assert_eq!(Candidates::<T>::contains_key(membership_id), true);

        assert_eq!(Candidates::<T>::get(membership_id).stake, amount);
    }

    pub fn check_candidacy_note(membership_id: &T::MembershipId, note: Option<&[u8]>) {
        assert_eq!(Candidates::<T>::contains_key(membership_id), true);

        let note_hash = match note {
            Some(tmp_note) => Some(T::Hashing::hash(tmp_note)),
            None => None,
        };

        assert_eq!(Candidates::<T>::get(membership_id).note_hash, note_hash,);
    }

    pub fn check_budget_refill(expected_balance: Balance<T>, expected_next_refill: T::BlockNumber) {
        assert_eq!(Budget::<T>::get(), expected_balance,);
        assert_eq!(NextBudgetRefill::<T>::get(), expected_next_refill,);
    }

    pub fn set_candidacy_note(
        origin: OriginType<T::AccountId>,
        membership_id: T::MembershipId,
        note: &[u8],
        expected_result: Result<(), Error<T>>,
    ) {
        // check method returns expected result
        assert_eq!(
            Module::<T>::set_candidacy_note(
                InstanceMockUtils::<T>::mock_origin(origin),
                membership_id,
                note.to_vec()
            ),
            expected_result,
        );

        if expected_result.is_err() {
            return;
        }
        assert_eq!(
            frame_system::Module::<Runtime>::events()
                .last()
                .unwrap()
                .event,
            TestEvent::event_mod(RawEvent::CandidacyNoteSet(
                membership_id.into().into(),
                note.into()
            )),
        );

        Self::check_candidacy_note(&membership_id, Some(note));
    }

    pub fn announce_candidacy(
        origin: OriginType<T::AccountId>,
        member_id: T::MembershipId,
        stake: Balance<T>,
        expected_result: Result<(), Error<T>>,
    ) {
        // use member id as staking and reward accounts
        Self::announce_candidacy_raw(
            origin,
            member_id,
            member_id.into(),
            member_id.into(),
            stake,
            expected_result,
        );
    }

    pub fn announce_candidacy_raw(
        origin: OriginType<T::AccountId>,
        member_id: T::MembershipId,
        staking_account_id: T::AccountId,
        reward_account_id: T::AccountId,
        stake: Balance<T>,
        expected_result: Result<(), Error<T>>,
    ) {
        // check method returns expected result
        assert_eq!(
            Module::<T>::announce_candidacy(
                InstanceMockUtils::<T>::mock_origin(origin),
                member_id,
                staking_account_id,
                reward_account_id,
                stake
            ),
            expected_result,
        );

        if expected_result.is_err() {
            return;
        }

        assert_eq!(
            frame_system::Module::<Runtime>::events()
                .last()
                .unwrap()
                .event,
            TestEvent::event_mod(RawEvent::NewCandidate(
                member_id.into().into(),
                stake.into()
            )),
        );
    }

    pub fn withdraw_candidacy(
        origin: OriginType<T::AccountId>,
        member_id: T::MembershipId,
        expected_result: Result<(), Error<T>>,
    ) {
        // check method returns expected result
        assert_eq!(
            Module::<T>::withdraw_candidacy(InstanceMockUtils::<T>::mock_origin(origin), member_id,),
            expected_result,
        );

        if expected_result.is_err() {
            return;
        }

        assert_eq!(
            frame_system::Module::<Runtime>::events()
                .last()
                .unwrap()
                .event,
            TestEvent::event_mod(RawEvent::CandidacyWithdraw(member_id.into().into(),)),
        );
    }

    pub fn release_candidacy_stake(
        origin: OriginType<T::AccountId>,
        member_id: T::MembershipId,
        expected_result: Result<(), Error<T>>,
    ) {
        // check method returns expected result
        assert_eq!(
            Module::<T>::release_candidacy_stake(
                InstanceMockUtils::<T>::mock_origin(origin),
                member_id,
            ),
            expected_result,
        );

        if expected_result.is_err() {
            return;
        }

        assert_eq!(
            frame_system::Module::<Runtime>::events()
                .last()
                .unwrap()
                .event,
            TestEvent::event_mod(RawEvent::CandidacyStakeRelease(member_id.into().into(),)),
        );
    }

    pub fn vote_for_candidate(
        origin: OriginType<T::AccountId>,
        commitment: T::Hash,
        stake: Balance<T>,
        expected_result: Result<(), ()>,
    ) -> () {
        // check method returns expected result
        assert_eq!(
            referendum::Module::<Runtime, ReferendumInstance>::vote(
                InstanceMockUtils::<T>::mock_origin(origin).into(),
                commitment.into(),
                stake.into(),
            )
            .is_ok(),
            expected_result.is_ok(),
        );
    }

    pub fn reveal_vote(
        origin: OriginType<T::AccountId>,
        salt: Vec<u8>,
        vote_option: u64,
        //expected_result: Result<(), referendum::Error<T, ReferendumInstance>>,
        expected_result: Result<(), ()>,
    ) -> () {
        // check method returns expected result
        assert_eq!(
            referendum::Module::<Runtime, ReferendumInstance>::reveal_vote(
                InstanceMockUtils::<T>::mock_origin(origin).into(),
                salt,
                vote_option,
            )
            .is_ok(),
            expected_result.is_ok(),
        );
    }

    pub fn release_vote_stake(
        origin: OriginType<<Runtime as frame_system::Trait>::AccountId>,
        expected_result: Result<(), ()>,
    ) -> () {
        // check method returns expected result
        assert_eq!(
            referendum::Module::<Runtime, ReferendumInstance>::release_vote_stake(
                InstanceMockUtils::<Runtime>::mock_origin(origin),
            )
            .is_ok(),
            expected_result.is_ok(),
        );
    }

    pub fn set_budget(
        origin: OriginType<T::AccountId>,
        amount: Balance<T>,
        expected_result: Result<(), ()>,
    ) {
        // check method returns expected result
        assert_eq!(
            Module::<T>::set_budget(InstanceMockUtils::<T>::mock_origin(origin), amount,).is_ok(),
            expected_result.is_ok(),
        );

        if expected_result.is_err() {
            return;
        }

        assert_eq!(Budget::<T>::get(), amount,);

        assert_eq!(
            frame_system::Module::<Runtime>::events()
                .last()
                .unwrap()
                .event,
            TestEvent::event_mod(RawEvent::BudgetBalanceSet(amount.into())),
        );
    }

    pub fn plan_budget_refill(
        origin: OriginType<T::AccountId>,
        next_refill: T::BlockNumber,
        expected_result: Result<(), ()>,
    ) {
        // check method returns expected result
        assert_eq!(
            Module::<T>::plan_budget_refill(
                InstanceMockUtils::<T>::mock_origin(origin),
                next_refill,
            )
            .is_ok(),
            expected_result.is_ok(),
        );

        if expected_result.is_err() {
            return;
        }

        assert_eq!(NextBudgetRefill::<T>::get(), next_refill,);

        assert_eq!(
            frame_system::Module::<Runtime>::events()
                .last()
                .unwrap()
                .event,
            TestEvent::event_mod(RawEvent::BudgetRefillPlanned(next_refill.into())),
        );
    }

    // simulate one council's election cycle
    pub fn simulate_council_cycle(params: CouncilCycleParams<T>) {
        let settings = params.council_settings;

        // check initial council members
        Self::check_council_members(params.expected_initial_council_members.clone());

        // start announcing
        Self::check_announcing_period(
            params.cycle_start_block_number,
            CouncilStageAnnouncing {
                candidates_count: 0,
            },
        );

        escape_checkpoint!(
            params.interrupt_point.clone(),
            Some(CouncilCycleInterrupt::BeforeCandidatesAnnounce)
        );

        // announce candidacy for each candidate
        params.candidates_announcing.iter().for_each(|candidate| {
            Self::announce_candidacy(
                candidate.origin.clone(),
                candidate.account_id.clone(),
                settings.min_candidate_stake,
                Ok(()),
            );
        });

        escape_checkpoint!(
            params.interrupt_point.clone(),
            Some(CouncilCycleInterrupt::AfterCandidatesAnnounce)
        );

        // forward to election-voting period
        InstanceMockUtils::<T>::increase_block_number(
            settings.announcing_stage_duration.into() + 1,
        );

        // finish announcing period / start referendum -> will cause period prolongement
        Self::check_election_period(
            params.cycle_start_block_number + settings.announcing_stage_duration,
            CouncilStageElection {
                candidates_count: params.expected_candidates.len() as u64,
            },
        );

        escape_checkpoint!(
            params.interrupt_point.clone(),
            Some(CouncilCycleInterrupt::BeforeVoting)
        );

        // vote with all voters
        params.voters.iter().for_each(|voter| {
            Self::vote_for_candidate(
                voter.origin.clone(),
                voter.commitment.clone(),
                voter.stake.clone(),
                Ok(()),
            )
        });

        escape_checkpoint!(
            params.interrupt_point.clone(),
            Some(CouncilCycleInterrupt::AfterVoting)
        );

        // forward to election-revealing period
        InstanceMockUtils::<T>::increase_block_number(settings.voting_stage_duration.into() + 1);

        // referendum - start revealing period
        Self::check_referendum_revealing(
            settings.council_size,
            vec![],
            BTreeMap::new(), //<u64, T::VotePower>,
            params.cycle_start_block_number
                + settings.announcing_stage_duration
                + settings.voting_stage_duration,
        );

        escape_checkpoint!(
            params.interrupt_point.clone(),
            Some(CouncilCycleInterrupt::BeforeRevealing)
        );

        // reveal vote for all voters
        params.voters.iter().for_each(|voter| {
            Self::reveal_vote(
                voter.origin.clone(),
                voter.salt.clone(),
                voter.vote_for,
                Ok(()),
            );
        });

        escape_checkpoint!(
            params.interrupt_point.clone(),
            Some(CouncilCycleInterrupt::AfterRevealing)
        );

        // finish election / start idle period
        InstanceMockUtils::<T>::increase_block_number(settings.reveal_stage_duration.into() + 1);
        Self::check_idle_period(
            params.cycle_start_block_number
                + settings.reveal_stage_duration
                + settings.announcing_stage_duration
                + settings.voting_stage_duration,
        );
        Self::check_council_members(params.expected_final_council_members.clone());

        // finish idle period
        InstanceMockUtils::<T>::increase_block_number(settings.idle_stage_duration.into() + 1);
    }

    // Simulate one full round of council lifecycle (announcing, election, idle). Use it to
    // quickly test behavior in 2nd, 3rd, etc. cycle.
    pub fn run_full_council_cycle(
        start_block_number: T::BlockNumber,
        expected_initial_council_members: &[CouncilMemberOf<T>],
        users_offset: u64,
    ) -> CouncilCycleParams<T> {
        let council_settings = CouncilSettings::<T>::extract_settings();
        let vote_stake = <Runtime as referendum::Trait<ReferendumInstance>>::MinimumStake::get();

        // generate candidates
        let candidates: Vec<CandidateInfo<T>> = (0..(council_settings.min_candidate_count + 1)
            as u64)
            .map(|i| {
                InstanceMockUtils::<T>::generate_candidate(
                    u64::from(i) + users_offset,
                    council_settings.min_candidate_stake,
                )
            })
            .collect();

        // prepare candidates that are expected to get into candidacy list
        let expected_candidates = candidates
            .iter()
            .map(|item| item.candidate.clone())
            .collect();

        let expected_final_council_members: Vec<CouncilMemberOf<T>> = vec![
            (
                candidates[3].candidate.clone(),
                candidates[3].membership_id,
                start_block_number + council_settings.election_duration - 1.into(),
                0.into(),
            )
                .into(),
            (
                candidates[0].candidate.clone(),
                candidates[0].membership_id,
                start_block_number + council_settings.election_duration - 1.into(),
                0.into(),
            )
                .into(),
            (
                candidates[1].candidate.clone(),
                candidates[1].membership_id,
                start_block_number + council_settings.election_duration - 1.into(),
                0.into(),
            )
                .into(),
        ];

        // generate voter for each 6 voters and give: 4 votes for option D, 3 votes for option A,
        // and 2 vote for option B, and 1 for option C
        let votes_map: Vec<u64> = vec![3, 3, 3, 3, 0, 0, 0, 1, 1, 2];
        let voters = (0..votes_map.len())
            .map(|index| {
                InstanceMockUtils::<T>::generate_voter(
                    index as u64 + users_offset,
                    vote_stake.into(),
                    CANDIDATE_BASE_ID + votes_map[index] + users_offset,
                    AnnouncementPeriodNr::get(),
                )
            })
            .collect();

        let params = CouncilCycleParams {
            council_settings: CouncilSettings::<T>::extract_settings(),
            cycle_start_block_number: start_block_number,
            expected_initial_council_members: expected_initial_council_members.to_vec(),
            expected_final_council_members,
            candidates_announcing: candidates.clone(),
            expected_candidates,
            voters,

            interrupt_point: None,
        };

        InstanceMocks::<T>::simulate_council_cycle(params.clone());

        params
    }
}
