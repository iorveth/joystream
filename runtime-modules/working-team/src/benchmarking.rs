#![cfg(feature = "runtime-benchmarks")]
use super::*;
use core::cmp::min;
use core::convert::TryInto;
use frame_benchmarking::{account, benchmarks_instance, Zero};
use frame_support::traits::OnInitialize;
use sp_runtime::traits::Bounded;
use sp_std::prelude::*;
use system as frame_system;
use system::EventRecord;
use system::Module as System;
use system::RawOrigin;

use crate::types::StakeParameters;
use crate::Module as WorkingTeam;
use membership::Module as Membership;

const SEED: u32 = 0;

enum StakingRole {
    WithStakes,
    WithoutStakes,
}

fn assert_last_event<T: Trait<I>, I: Instance>(generic_event: <T as Trait<I>>::Event) {
    let events = System::<T>::events();
    let system_event: <T as frame_system::Trait>::Event = generic_event.into();
    // compare to the last event record
    let EventRecord { event, .. } = &events[events.len() - 1];
    assert_eq!(event, &system_event);
}

fn get_byte(num: u32, byte_number: u8) -> u8 {
    ((num & (0xff << (8 * byte_number))) >> 8 * byte_number) as u8
}

fn add_opening_helper<T: Trait<I>, I: Instance>(
    id: u32,
    add_opening_origin: &T::Origin,
    staking_role: &StakingRole,
    job_opening_type: &JobOpeningType,
) -> T::OpeningId {
    let staking_policy = match staking_role {
        StakingRole::WithStakes => Some(StakePolicy {
            stake_amount: One::one(),
            leaving_unstaking_period: T::MinUnstakingPeriodLimit::get() + One::one(),
        }),
        StakingRole::WithoutStakes => None,
    };

    WorkingTeam::<T, _>::add_opening(
        add_opening_origin.clone(),
        vec![],
        *job_opening_type,
        staking_policy,
        Some(RewardPolicy {
            reward_per_block: One::one(),
        }),
    )
    .unwrap();

    let opening_id = T::OpeningId::from(id.try_into().unwrap());

    assert!(
        OpeningById::<T, I>::contains_key(opening_id),
        "Opening not added"
    );

    opening_id
}

fn apply_on_opening_helper<T: Trait<I>, I: Instance>(
    id: u32,
    staking_role: &StakingRole,
    applicant_id: &T::AccountId,
    member_id: &T::MemberId,
    opening_id: &T::OpeningId,
) -> T::ApplicationId {
    let stake_parameters = match staking_role {
        StakingRole::WithStakes => Some(StakeParameters {
            // Due to mock implementation of StakingHandler we can't go over 1000
            stake: min(
                BalanceOfCurrency::<T>::max_value(),
                BalanceOfCurrency::<T>::from(1000),
            ),
            staking_account_id: applicant_id.clone(),
        }),
        StakingRole::WithoutStakes => None,
    };

    WorkingTeam::<T, _>::apply_on_opening(
        RawOrigin::Signed(applicant_id.clone()).into(),
        ApplyOnOpeningParameters::<T, I> {
            member_id: *member_id,
            opening_id: *opening_id,
            role_account_id: applicant_id.clone(),
            reward_account_id: applicant_id.clone(),
            description: vec![],
            stake_parameters,
        },
    )
    .unwrap();

    let application_id = T::ApplicationId::from(id.try_into().unwrap());

    assert!(
        ApplicationById::<T, I>::contains_key(application_id),
        "Application not added"
    );

    application_id
}

fn add_opening_and_n_apply<T: Trait<I>, I: Instance>(
    ids: &Vec<u32>,
    add_opening_origin: &T::Origin,
    staking_role: &StakingRole,
    job_opening_type: &JobOpeningType,
) -> (T::OpeningId, BTreeSet<T::ApplicationId>, Vec<T::AccountId>) {
    let opening_id =
        add_opening_helper::<T, I>(1, add_opening_origin, &staking_role, job_opening_type);

    let mut successful_application_ids = BTreeSet::new();

    let mut account_ids = Vec::new();
    for id in ids.iter() {
        let (applicant_account_id, applicant_member_id) = member_funded_account::<T>("member", *id);
        let application_id = apply_on_opening_helper::<T, I>(
            *id,
            &staking_role,
            &applicant_account_id,
            &applicant_member_id,
            &opening_id,
        );

        successful_application_ids.insert(application_id);
        account_ids.push(applicant_account_id);
    }

    (opening_id, successful_application_ids, account_ids)
}

fn add_and_apply_opening<T: Trait<I>, I: Instance>(
    id: u32,
    add_opening_origin: &T::Origin,
    staking_role: &StakingRole,
    applicant_id: &T::AccountId,
    member_id: &T::MemberId,
    job_opening_type: &JobOpeningType,
) -> (T::OpeningId, T::ApplicationId) {
    let opening_id =
        add_opening_helper::<T, I>(id, add_opening_origin, staking_role, job_opening_type);

    let application_id =
        apply_on_opening_helper::<T, I>(id, staking_role, applicant_id, member_id, &opening_id);

    (opening_id, application_id)
}

// Method to generate a distintic valid handle
// for a membership. For each index.
// TODO: This will only work as long as max_handle_length >= 4
fn handle_from_id<T: membership::Trait>(id: u32) -> Vec<u8> {
    let min_handle_length = Membership::<T>::min_handle_length();
    // If the index is ever different from u32 change this
    let mut handle = vec![
        get_byte(id, 0),
        get_byte(id, 1),
        get_byte(id, 2),
        get_byte(id, 3),
    ];

    while handle.len() < (min_handle_length as usize) {
        handle.push(0u8);
    }

    handle
}

fn member_funded_account<T: membership::Trait>(
    name: &'static str,
    id: u32,
) -> (T::AccountId, T::MemberId) {
    let account_id = account::<T::AccountId>(name, id, SEED);
    let handle = handle_from_id::<T>(id);

    let _ = <T as common::currency::GovernanceCurrency>::Currency::make_free_balance_be(
        &account_id,
        BalanceOfCurrency::<T>::max_value(),
    );

    let authority_account = account::<T::AccountId>(name, 0, SEED);

    Membership::<T>::set_screening_authority(RawOrigin::Root.into(), authority_account.clone())
        .unwrap();

    Membership::<T>::add_screened_member(
        RawOrigin::Signed(authority_account.clone()).into(),
        account_id.clone(),
        Some(handle),
        None,
        None,
    )
    .unwrap();

    (account_id, T::MemberId::from(id.try_into().unwrap()))
}

fn force_missed_reward<T: Trait<I>, I: Instance>() {
    let curr_block_number =
        System::<T>::block_number().saturating_add(T::RewardPeriod::get().into());
    System::<T>::set_block_number(curr_block_number);
    WorkingTeam::<T, _>::set_budget(RawOrigin::Root.into(), Zero::zero()).unwrap();
    WorkingTeam::<T, _>::on_initialize(curr_block_number);
}

fn insert_a_worker<T: Trait<I>, I: Instance>(
    staking_role: StakingRole,
    job_opening_type: JobOpeningType,
    id: u32,
    lead_id: Option<T::AccountId>,
) -> (T::AccountId, TeamWorkerId<T>)
where
    WorkingTeam<T, I>: OnInitialize<T::BlockNumber>,
{
    let add_worker_origin = match job_opening_type {
        JobOpeningType::Leader => RawOrigin::Root,
        JobOpeningType::Regular => RawOrigin::Signed(lead_id.clone().unwrap()),
    };

    let (caller_id, member_id) = member_funded_account::<T>("member", id);

    let (opening_id, application_id) = add_and_apply_opening::<T, I>(
        id,
        &T::Origin::from(add_worker_origin.clone()),
        &staking_role,
        &caller_id,
        &member_id,
        &job_opening_type,
    );

    let mut successful_application_ids = BTreeSet::<T::ApplicationId>::new();
    successful_application_ids.insert(application_id);
    WorkingTeam::<T, _>::fill_opening(
        add_worker_origin.clone().into(),
        opening_id,
        successful_application_ids,
    )
    .unwrap();

    // Every worst case either include or doesn't mind having a non-zero
    // remaining reward
    force_missed_reward::<T, I>();

    let worker_id = TeamWorkerId::<T>::from(id.try_into().unwrap());

    assert!(WorkerById::<T, I>::contains_key(worker_id));

    (caller_id, worker_id)
}

benchmarks_instance! {
    _ { }

    on_initialize_leaving {
      let i in 1 .. T::MaxWorkerNumberLimit::get();

      let (lead_id, lead_worker_id) = insert_a_worker::<T, I>(StakingRole::WithStakes, JobOpeningType::Leader, 0, None);

      let (opening_id, successful_application_ids, application_account_id) = add_opening_and_n_apply::<T, I>(
        &(1..i).collect(),
        &T::Origin::from(RawOrigin::Signed(lead_id.clone())),
        &StakingRole::WithStakes,
        &JobOpeningType::Regular
      );

      WorkingTeam::<T, _>::fill_opening(RawOrigin::Signed(lead_id.clone()).into(), opening_id,
      successful_application_ids.clone()).unwrap();


      force_missed_reward::<T,I>();

      // Force all workers to leave (Including the lead)
      // We should have every TeamWorkerId from 0 to i-1
      // Corresponding to each account id
      let mut worker_id = Zero::zero();
      for id in application_account_id {
        worker_id += One::one();
        WorkingTeam::<T, _>::leave_role(RawOrigin::Signed(id).into(), worker_id).unwrap();
      }

      // Worst case scenario one of the leaving workers is the lead
      WorkingTeam::<T, _>::leave_role(RawOrigin::Signed(lead_id).into(), lead_worker_id).unwrap();

      for i in 1..successful_application_ids.len() {
        let worker = TeamWorkerId::<T>::from(i.try_into().unwrap());
        assert!(WorkerById::<T, I>::contains_key(worker), "Not all workers
        added");
        assert_eq!(WorkingTeam::<T, _>::worker_by_id(worker).started_leaving_at, Some(System::<T>::block_number()), "Worker hasn't started leaving");
      }

      // Maintain consistency with add_opening_helper
      let leaving_unstaking_period = T::MinUnstakingPeriodLimit::get() + One::one();

      // Force unstaking period to have passed
      let curr_block_number =
          System::<T>::block_number().saturating_add(leaving_unstaking_period.into());
      System::<T>::set_block_number(curr_block_number);
      WorkingTeam::<T, _>::set_budget(RawOrigin::Root.into(), BalanceOfCurrency::<T>::max_value()).unwrap();
      assert_eq!(WorkingTeam::<T, _>::budget(), BalanceOfCurrency::<T>::max_value());
    }: { WorkingTeam::<T, _>::on_initialize(curr_block_number) }
    verify {
      let reward_per_worker = BalanceOfCurrency::<T>::from(T::RewardPeriod::get());
      WorkerById::<T, I>::iter().for_each(|(worker_id, _)| {
        assert!(!WorkerById::<T, I>::contains_key(worker_id), "Worker hasn't left");
      });
      assert_eq!(WorkingTeam::<T, I>::budget(), BalanceOfCurrency::<T>::max_value().saturating_sub(BalanceOfCurrency::<T>::from(i) * reward_per_worker).saturating_sub(reward_per_worker), "Budget wasn't correctly updated, probably not all workers rewarded");
    }


    on_initialize_rewarding_with_missing_reward {
      let i in 1 .. T::MaxWorkerNumberLimit::get();

      let (lead_id, _) = insert_a_worker::<T, I>(StakingRole::WithStakes, JobOpeningType::Leader, 0, None);

      let (opening_id, successful_application_ids, _) = add_opening_and_n_apply::<T, I>(
        &(1..i).collect(),
        &T::Origin::from(RawOrigin::Signed(lead_id.clone())),
        &StakingRole::WithStakes,
        &JobOpeningType::Regular
      );

      WorkingTeam::<T, _>::fill_opening(RawOrigin::Signed(lead_id.clone()).into(), opening_id,
      successful_application_ids.clone()).unwrap();

      for i in 1..successful_application_ids.len() {
        assert!(WorkerById::<T, I>::contains_key(TeamWorkerId::<T>::from(i.try_into().unwrap())), "Not all workers
        added");
      }

      // Worst case scenario there is a missing reward
      force_missed_reward::<T, I>();

      // Sets periods so that we can reward
      let curr_block_number =
          System::<T>::block_number().saturating_add(T::RewardPeriod::get().into());
      System::<T>::set_block_number(curr_block_number);

      // Sets budget so that we can pay it
      WorkingTeam::<T, _>::set_budget(RawOrigin::Root.into(), BalanceOfCurrency::<T>::max_value()).unwrap();
      assert_eq!(WorkingTeam::<T, _>::budget(), BalanceOfCurrency::<T>::max_value());

    }: { WorkingTeam::<T, _>::on_initialize(curr_block_number) }
    verify {
      let reward_per_worker = BalanceOfCurrency::<T>::from(T::RewardPeriod::get());
      assert_eq!(WorkingTeam::<T, _>::budget(),
      // When creating a worker using `insert_a_worker` it gives the lead a number of block equating to
      // reward period as missed reward(and the reward value is 1) therefore the additional discount of
      // balance
      BalanceOfCurrency::<T>::max_value().saturating_sub(BalanceOfCurrency::<T>::from(i) * reward_per_worker * BalanceOfCurrency::<T>::from(2)).saturating_sub(reward_per_worker),
      "Budget wasn't correctly updated, probably not all workers rewarded");
    }

    on_initialize_rewarding_with_missing_reward_cant_pay {
      let i in 1 .. T::MaxWorkerNumberLimit::get();

      let (lead_id, _) = insert_a_worker::<T, I>(StakingRole::WithStakes, JobOpeningType::Leader, 0, None);

      let (opening_id, successful_application_ids, _) = add_opening_and_n_apply::<T, I>(
        &(1..i).collect(),
        &T::Origin::from(RawOrigin::Signed(lead_id.clone())),
        &StakingRole::WithStakes,
        &JobOpeningType::Regular
      );

      WorkingTeam::<T, _>::fill_opening(RawOrigin::Signed(lead_id.clone()).into(), opening_id,
      successful_application_ids.clone()).unwrap();

      for i in 1..successful_application_ids.len() {
        assert!(WorkerById::<T, I>::contains_key(TeamWorkerId::<T>::from(i.try_into().unwrap())), "Not all workers
        added");
      }

      // Sets periods so that we can reward
      let curr_block_number =
          System::<T>::block_number().saturating_add(T::RewardPeriod::get().into());
      System::<T>::set_block_number(curr_block_number);

      // Sets budget so that we can't pay it
      WorkingTeam::<T, _>::set_budget(RawOrigin::Root.into(), Zero::zero()).unwrap();
      assert_eq!(WorkingTeam::<T, _>::budget(), Zero::zero());

    }: { WorkingTeam::<T, _>::on_initialize(curr_block_number) }
    verify {
      WorkerById::<T, I>::iter().for_each(|(_, worker)| {
        assert!(worker.missed_reward.expect("There should be some missed reward") >= BalanceOfCurrency::<T>::from(T::RewardPeriod::get()), "At least one worker wasn't
        rewarded");
      });
    }

    on_initialize_rewarding_without_missing_reward {
      let i in 1 .. T::MaxWorkerNumberLimit::get();

      let (lead_id, _) = insert_a_worker::<T, I>(StakingRole::WithStakes, JobOpeningType::Leader, 0, None);

      let (opening_id, successful_application_ids, _) = add_opening_and_n_apply::<T, I>(
        &(1..i).collect(),
        &T::Origin::from(RawOrigin::Signed(lead_id.clone())),
        &StakingRole::WithStakes,
        &JobOpeningType::Regular
      );

      WorkingTeam::<T, _>::fill_opening(RawOrigin::Signed(lead_id.clone()).into(), opening_id,
      successful_application_ids.clone()).unwrap();

      for i in 1..successful_application_ids.len() {
        assert!(WorkerById::<T, I>::contains_key(TeamWorkerId::<T>::from(i.try_into().unwrap())), "Not all workers
        added");
      }

      // Sets periods so that we can reward
      let curr_block_number =
          System::<T>::block_number().saturating_add(T::RewardPeriod::get().into());
      System::<T>::set_block_number(curr_block_number);

      // Sets budget so that we can pay it
      WorkingTeam::<T, _>::set_budget(RawOrigin::Root.into(), BalanceOfCurrency::<T>::max_value()).unwrap();
      assert_eq!(WorkingTeam::<T, _>::budget(), BalanceOfCurrency::<T>::max_value());

    }: { WorkingTeam::<T, _>::on_initialize(curr_block_number) }
    verify {
      let reward_per_worker = BalanceOfCurrency::<T>::from(T::RewardPeriod::get());
      assert_eq!(WorkingTeam::<T, _>::budget(),
      // When creating a worker using `insert_a_worker` it gives the lead a number of block equating to
      // reward period as missed reward(and the reward value is 1) therefore the additional discount of
      // balance
      BalanceOfCurrency::<T>::max_value().saturating_sub(BalanceOfCurrency::<T>::from(i) * reward_per_worker).saturating_sub(reward_per_worker),
      "Budget wasn't correctly updated, probably not all workers rewarded");
    }

    apply_on_opening {
      let i in 1 .. 50000;

      let (lead_account_id, lead_member_id) = member_funded_account::<T>("lead", 0);
      let opening_id = add_opening_helper::<T, I>(0, &T::Origin::from(RawOrigin::Root), &StakingRole::WithStakes, &JobOpeningType::Leader);

      let apply_on_opening_params = ApplyOnOpeningParameters::<T, I> {
        member_id: lead_member_id,
        opening_id: opening_id.clone(),
        role_account_id: lead_account_id.clone(),
        reward_account_id: lead_account_id.clone(),
        description: vec![0u8; i.try_into().unwrap()],
        stake_parameters: Some(
          // Make sure to keep consistency with the StakePolicy in add_opening_helper (we are safe as long as we are
          // using max_value for stake)
          StakeParameters {
            stake: One::one(),
            staking_account_id: lead_account_id.clone(),
          }
        ),
      };

    }: _ (RawOrigin::Signed(lead_account_id.clone()), apply_on_opening_params)
    verify {
      assert!(ApplicationById::<T, I>::contains_key(T::ApplicationId::from(0)), "Application not found");
      assert_last_event::<T, I>(RawEvent::AppliedOnOpening(opening_id, Zero::zero()).into());
    }

    fill_opening_lead {
      let i in 0 .. 10;

      let (lead_account_id, lead_member_id) = member_funded_account::<T>("lead", 0);
      let (opening_id, application_id) = add_and_apply_opening::<T, I>(0, &RawOrigin::Root.into(), &StakingRole::WithoutStakes, &lead_account_id,
        &lead_member_id, &JobOpeningType::Leader);

      let mut successful_application_ids: BTreeSet<T::ApplicationId> = BTreeSet::new();
      successful_application_ids.insert(application_id);
    }: fill_opening(RawOrigin::Root, opening_id, successful_application_ids)
    verify {
      assert!(!OpeningById::<T, I>::contains_key(opening_id), "Opening still not filled");
      assert_eq!(WorkingTeam::<T, I>::current_lead(), Some(Zero::zero()), "Opening for lead not filled");
    }

    fill_opening_worker { // We can actually fill an opening with 0 applications?
      let i in 1 .. T::MaxWorkerNumberLimit::get();
      let (lead_id, lead_worker_id) = insert_a_worker::<T, I>(StakingRole::WithoutStakes, JobOpeningType::Leader, 0, None);

      let (opening_id, successful_application_ids, _) = add_opening_and_n_apply::<T, I>(
        &(1..i).collect(),
        &T::Origin::from(RawOrigin::Signed(lead_id.clone())),
        &StakingRole::WithoutStakes,
        &JobOpeningType::Regular
      );
    }: fill_opening(RawOrigin::Signed(lead_id.clone()), opening_id, successful_application_ids.clone())
    verify {
      assert!(!OpeningById::<T, I>::contains_key(opening_id), "Opening still not filled");

      for i in 1..successful_application_ids.len() {
        assert!(WorkerById::<T, I>::contains_key(TeamWorkerId::<T>::from(i.try_into().unwrap())), "Not all workers
        added");
      }

    }

    update_role_account{
      let i in 1 .. 10;
      let (lead_id, lead_worker_id) = insert_a_worker::<T, I>(StakingRole::WithoutStakes, JobOpeningType::Leader, 0, None);
      let new_account_id = account::<T::AccountId>("new_lead_account", 1, SEED);
    }: _ (RawOrigin::Signed(lead_id), lead_worker_id, new_account_id.clone())
    verify {
      assert_eq!(WorkingTeam::<T, I>::worker_by_id(lead_worker_id).role_account_id, new_account_id, "Role account not
      updated");
      assert_last_event::<T, I>(RawEvent::WorkerRoleAccountUpdated(lead_worker_id, new_account_id).into());
    }

    cancel_opening {
      let i in 1 .. 10;

      let (lead_id, _) = insert_a_worker::<T, I>(StakingRole::WithoutStakes, JobOpeningType::Leader, 0, None);
      let opening_id = add_opening_helper::<T, I>(
        1,
        &T::Origin::from(RawOrigin::Signed(lead_id.clone())),
        &StakingRole::WithoutStakes,
        &JobOpeningType::Regular
      );

    }: _ (RawOrigin::Signed(lead_id.clone()), opening_id)
    verify {
      assert!(!OpeningById::<T, I>::contains_key(opening_id), "Opening not removed");
      assert_last_event::<T, I>(RawEvent::OpeningCanceled(opening_id).into());
    }

    withdraw_application {
      let i in 1 .. 10;

      let (caller_id, member_id) = member_funded_account::<T>("lead", 0);
      let (_, application_id) = add_and_apply_opening::<T, I>(0,
        &RawOrigin::Root.into(),
        &StakingRole::WithStakes,
        &caller_id,
        &member_id,
        &JobOpeningType::Leader
        );

    }: _ (RawOrigin::Signed(caller_id.clone()), application_id)
    verify {
      assert!(!ApplicationById::<T, I>::contains_key(application_id), "Application not removed");
      assert_last_event::<T, I>(RawEvent::ApplicationWithdrawn(application_id).into());
    }

    // Regular worker is the worst case scenario since the checks
    // require access to the storage whilist that's not the case with a lead opening
    slash_stake {
      let i in 0 .. 10;

      let (lead_id, lead_worker_id) = insert_a_worker::<T, I>(StakingRole::WithoutStakes, JobOpeningType::Leader, 0, None);
      let (caller_id, worker_id) = insert_a_worker::<T, I>(StakingRole::WithStakes, JobOpeningType::Regular, 1, Some(lead_id.clone()));
      let slashing_amount = One::one();
      let penalty = Penalty {
        slashing_text: vec![],
        slashing_amount,
      };
    }: _(RawOrigin::Signed(lead_id.clone()), worker_id, penalty)
    verify {
      assert_last_event::<T, I>(RawEvent::StakeSlashed(worker_id, slashing_amount).into());
    }

    terminate_role_worker {
      let i in 0 .. 10;

      let (lead_id, _) = insert_a_worker::<T, I>(StakingRole::WithoutStakes, JobOpeningType::Leader, 0, None);
      let (caller_id, worker_id) = insert_a_worker::<T, I>(StakingRole::WithStakes, JobOpeningType::Regular, 1, Some(lead_id.clone()));
      // To be able to pay unpaid reward
      let current_budget = BalanceOfCurrency::<T>::max_value();
      WorkingTeam::<T, _>::set_budget(RawOrigin::Root.into(), current_budget).unwrap();
      let penalty = Penalty {
        slashing_text: vec![],
        slashing_amount: One::one(),
      };
    }: terminate_role(RawOrigin::Signed(lead_id.clone()), worker_id, Some(penalty))
    verify {
      assert!(!WorkerById::<T, I>::contains_key(worker_id), "Worker not terminated");
      assert_last_event::<T, I>(RawEvent::TerminatedWorker(worker_id).into());
    }

    terminate_role_lead {
      let i in 0 .. 10;

      let (_, lead_worker_id) = insert_a_worker::<T, I>(StakingRole::WithStakes, JobOpeningType::Leader, 0, None);
      let current_budget = BalanceOfCurrency::<T>::max_value();
      // To be able to pay unpaid reward
      WorkingTeam::<T, _>::set_budget(RawOrigin::Root.into(), current_budget).unwrap();
      let penalty = Penalty {
        slashing_text: vec![],
        slashing_amount: One::one(),
      };
    }: terminate_role(RawOrigin::Root, lead_worker_id, Some(penalty))
    verify {
      assert!(!WorkerById::<T, I>::contains_key(lead_worker_id), "Worker not terminated");
      assert_last_event::<T, I>(RawEvent::TerminatedLeader(lead_worker_id).into());
    }

    // Regular worker is the worst case scenario since the checks
    // require access to the storage whilist that's not the case with a lead opening
    increase_stake {
      let i in 0 .. 10;

      let (lead_id, _) = insert_a_worker::<T, I>(StakingRole::WithoutStakes, JobOpeningType::Leader, 0, None);
      let (caller_id, worker_id) = insert_a_worker::<T, I>(StakingRole::WithStakes, JobOpeningType::Regular, 1, Some(lead_id.clone()));

      let old_stake = One::one();
      WorkingTeam::<T, _>::decrease_stake(RawOrigin::Signed(lead_id.clone()).into(), worker_id.clone(), old_stake).unwrap();
      let new_stake = old_stake + One::one();
    }: _ (RawOrigin::Signed(caller_id.clone()), worker_id.clone(), new_stake)
    verify {
      assert_last_event::<T, I>(RawEvent::StakeIncreased(worker_id, new_stake).into());
    }

    // Regular worker is the worst case scenario since the checks
    // require access to the storage whilist that's not the case with a lead opening
    decrease_stake {
      let i in 0 .. 10;

      let (lead_id, _) = insert_a_worker::<T, I>(StakingRole::WithoutStakes, JobOpeningType::Leader, 0, None);
      let (_, worker_id) = insert_a_worker::<T, I>(StakingRole::WithStakes, JobOpeningType::Regular, 1, Some(lead_id.clone()));

      // I'm assuming that we will usually have MaxBalance > 1
      let new_stake = One::one();
    }: _ (RawOrigin::Signed(lead_id), worker_id, new_stake)
    verify {
      assert_last_event::<T, I>(RawEvent::StakeDecreased(worker_id, new_stake).into());
    }

    spend_from_budget {
      let i in 0 .. 10;

      let (lead_id, _) = insert_a_worker::<T, I>(StakingRole::WithoutStakes, JobOpeningType::Leader, 0, None);

      let current_budget = BalanceOfCurrency::<T>::max_value();
      WorkingTeam::<T, _>::set_budget(RawOrigin::Root.into(), current_budget).unwrap();
    }: _ (RawOrigin::Signed(lead_id.clone()), lead_id.clone(), current_budget, None)
    verify {
      assert_eq!(WorkingTeam::<T, I>::budget(), Zero::zero(), "Budget not updated");
      assert_last_event::<T, I>(RawEvent::BudgetSpending(lead_id, current_budget).into());
    }

    // Regular worker is the worst case scenario since the checks
    // require access to the storage whilist that's not the case with a lead opening
    update_reward_amount {
      let i in 0 .. 10;

      let (lead_id, _) = insert_a_worker::<T, I>(StakingRole::WithoutStakes, JobOpeningType::Leader, 0, None);
      let (_, worker_id) = insert_a_worker::<T, I>(StakingRole::WithoutStakes, JobOpeningType::Regular, 1, Some(lead_id.clone()));
      let new_reward = Some(BalanceOfCurrency::<T>::max_value());
    }: _ (RawOrigin::Signed(lead_id.clone()), worker_id, new_reward)
    verify {
      assert_eq!(WorkingTeam::<T, I>::worker_by_id(worker_id).reward_per_block, new_reward, "Reward not updated");
      assert_last_event::<T, I>(RawEvent::WorkerRewardAmountUpdated(worker_id, new_reward).into());
    }

    set_status_text {
      let i in 0 .. 50000; // TODO: We should have a bounded value for description

      let (lead_id, _) = insert_a_worker::<T, I>(StakingRole::WithoutStakes, JobOpeningType::Leader, 0, None);
      let status_text = Some(vec![0u8; i.try_into().unwrap()]);

    }: _ (RawOrigin::Signed(lead_id), status_text.clone())
    verify {
      let status_text_hash = T::Hashing::hash(&status_text.unwrap()).as_ref().to_vec();
      assert_eq!(WorkingTeam::<T, I>::status_text_hash(), status_text_hash, "Status text not updated");
      assert_last_event::<T, I>(RawEvent::StatusTextChanged(status_text_hash).into());
    }

    update_reward_account {
      let i in 0 .. 10;

      let (caller_id, worker_id) = insert_a_worker::<T, I>(StakingRole::WithoutStakes, JobOpeningType::Leader, 0, None);
      let new_id = account::<T::AccountId>("new_id", 1, 0);

    }: _ (RawOrigin::Signed(caller_id), worker_id, new_id.clone())
    verify {
      assert_eq!(WorkingTeam::<T, I>::worker_by_id(worker_id).reward_account_id, new_id, "Reward account not updated");
      assert_last_event::<T, I>(RawEvent::WorkerRewardAccountUpdated(worker_id, new_id).into());
    }

    set_budget {
      let i in 0 .. 10;

      let new_budget = BalanceOfCurrency::<T>::max_value();

    }: _(RawOrigin::Root, new_budget)
    verify {
      assert_eq!(WorkingTeam::<T, I>::budget(), new_budget, "Budget isn't updated");
      assert_last_event::<T, I>(RawEvent::BudgetSet(new_budget).into());
    }

    // Regular opening is the worst case scenario since the checks
    // require access to the storage whilist that's not the case with a lead opening
    add_opening{
      let i in 0 .. 50000; // TODO: We should have a bounded value for description

      let (lead_id, _) = insert_a_worker::<T, I>(StakingRole::WithoutStakes, JobOpeningType::Leader, 0, None);


      let stake_policy = StakePolicy {
        stake_amount: BalanceOfCurrency::<T>::max_value(),
        leaving_unstaking_period: T::BlockNumber::max_value(),
      };

      let reward_policy = RewardPolicy {
        reward_per_block: BalanceOfCurrency::<T>::max_value(),
      };

      let description = vec![0u8; i.try_into().unwrap()];

    }: _(RawOrigin::Signed(lead_id), description, JobOpeningType::Regular, Some(stake_policy), Some(reward_policy))
    verify {
      assert!(OpeningById::<T, I>::contains_key(T::OpeningId::from(1)));
      assert_last_event::<T, I>(RawEvent::OpeningAdded(T::OpeningId::from(1)).into());
    }

    // This is always worse than leave_role_immediatly
    leave_role_immediatly {
        let i in 0 .. 10; // TODO: test not running if we don't set a range of values
        // Worst case scenario there is a lead(this requires **always** more steps)
        // could separate into new branch to tighten weight
        // Also, workers without stake can leave immediatly
        let (caller_id, lead_worker_id) = insert_a_worker::<T, I>(StakingRole::WithoutStakes, JobOpeningType::Leader, 0, None);

        // To be able to pay unpaid reward
        WorkingTeam::<T, _>::set_budget(RawOrigin::Root.into(), BalanceOfCurrency::<T>::max_value()).unwrap();
    }: leave_role(RawOrigin::Signed(caller_id), lead_worker_id)
    verify {
      assert!(!WorkerById::<T, I>::contains_key(lead_worker_id), "Worker hasn't left");
      assert_last_event::<T, I>(RawEvent::WorkerExited(lead_worker_id).into());
    }


    // Generally speaking this seems to be always the best case scenario
    // but since it's so obviously a different branch I think it's a good idea
    // to leave this branch and use tha max between these 2
    leave_role_later {
        let i in 0 .. 10;

        // Workers with stake can't leave immediatly
        let (caller_id, caller_worker_id) = insert_a_worker::<T, I>(StakingRole::WithStakes, JobOpeningType::Leader, 0, None);
    }: leave_role(RawOrigin::Signed(caller_id), caller_worker_id)
    verify {
      assert_eq!(WorkingTeam::<T, _>::worker_by_id(caller_worker_id).started_leaving_at, Some(System::<T>::block_number()), "Worker hasn't started leaving");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{build_test_externalities, Test};
    use frame_support::assert_ok;

    #[test]
    fn test_leave_role_later() {
        build_test_externalities().execute_with(|| {
            assert_ok!(test_benchmark_leave_role_later::<Test>());
        });
    }

    #[test]
    fn test_leave_role_immediatly() {
        build_test_externalities().execute_with(|| {
            assert_ok!(test_benchmark_leave_role_immediatly::<Test>());
        });
    }

    #[test]
    fn test_add_opening() {
        build_test_externalities().execute_with(|| {
            assert_ok!(test_benchmark_add_opening::<Test>());
        });
    }

    #[test]
    fn test_set_budget() {
        build_test_externalities().execute_with(|| {
            assert_ok!(test_benchmark_set_budget::<Test>());
        });
    }

    #[test]
    fn test_update_reward_account() {
        build_test_externalities().execute_with(|| {
            assert_ok!(test_benchmark_update_reward_account::<Test>());
        });
    }

    #[test]
    fn test_set_status_text() {
        build_test_externalities().execute_with(|| {
            assert_ok!(test_benchmark_set_status_text::<Test>());
        });
    }

    #[test]
    fn test_update_reward_amount() {
        build_test_externalities().execute_with(|| {
            assert_ok!(test_benchmark_update_reward_amount::<Test>());
        });
    }

    #[test]
    fn test_spend_from_budget() {
        build_test_externalities().execute_with(|| {
            assert_ok!(test_benchmark_spend_from_budget::<Test>());
        });
    }

    #[test]
    fn test_decrease_stake() {
        build_test_externalities().execute_with(|| {
            assert_ok!(test_benchmark_decrease_stake::<Test>());
        });
    }

    #[test]
    fn test_increase_stake() {
        build_test_externalities().execute_with(|| {
            assert_ok!(test_benchmark_increase_stake::<Test>());
        });
    }

    #[test]
    fn test_terminate_role_lead() {
        build_test_externalities().execute_with(|| {
            assert_ok!(test_benchmark_terminate_role_lead::<Test>());
        });
    }

    #[test]
    fn test_terminate_role_worker() {
        build_test_externalities().execute_with(|| {
            assert_ok!(test_benchmark_terminate_role_worker::<Test>());
        });
    }

    #[test]
    fn test_slash_stake() {
        build_test_externalities().execute_with(|| {
            assert_ok!(test_benchmark_slash_stake::<Test>());
        });
    }

    #[test]
    fn test_withdraw_application() {
        build_test_externalities().execute_with(|| {
            assert_ok!(test_benchmark_withdraw_application::<Test>());
        });
    }

    #[test]
    fn test_cancel_opening() {
        build_test_externalities().execute_with(|| {
            assert_ok!(test_benchmark_cancel_opening::<Test>());
        });
    }

    #[test]
    fn test_update_role_account() {
        build_test_externalities().execute_with(|| {
            assert_ok!(test_benchmark_update_role_account::<Test>());
        });
    }

    #[test]
    fn test_fill_opening_worker() {
        build_test_externalities().execute_with(|| {
            assert_ok!(test_benchmark_fill_opening_worker::<Test>());
        });
    }

    #[test]
    fn test_fill_opening_lead() {
        build_test_externalities().execute_with(|| {
            assert_ok!(test_benchmark_fill_opening_lead::<Test>());
        });
    }

    #[test]
    fn test_apply_on_opening() {
        build_test_externalities().execute_with(|| {
            assert_ok!(test_benchmark_apply_on_opening::<Test>());
        });
    }

    #[test]
    fn test_on_inintialize_rewarding_without_missing_reward() {
        build_test_externalities().execute_with(|| {
            assert_ok!(test_benchmark_on_initialize_rewarding_without_missing_reward::<Test>());
        });
    }

    #[test]
    fn test_on_inintialize_rewarding_with_missing_reward_cant_pay() {
        build_test_externalities().execute_with(|| {
            assert_ok!(
                test_benchmark_on_initialize_rewarding_with_missing_reward_cant_pay::<Test>()
            );
        });
    }

    #[test]
    fn test_on_inintialize_rewarding_with_missing_reward() {
        build_test_externalities().execute_with(|| {
            assert_ok!(test_benchmark_on_initialize_rewarding_with_missing_reward::<Test>());
        });
    }

    #[test]
    fn test_on_inintialize_leaving() {
        build_test_externalities().execute_with(|| {
            assert_ok!(test_benchmark_on_initialize_leaving::<Test>());
        });
    }
}
