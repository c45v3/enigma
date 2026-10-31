use super::*;

#[tokio::test]
async fn seedless_record_does_not_block_its_first_replayable_replacement() {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    database::run_migrations(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO dungeon_records
         (user_id, episode_id, record_round, hero_list, sub_hero_list, cloth_id, equips, version, created_at, oper_records)
         VALUES (9, 10101, 1, '[]', '[]', 1, '[]', 7, 0, '[]')",
    )
    .execute(&pool)
    .await
    .unwrap();

    assert_eq!(
        dungeons::dungeon_record_round(&pool, 9, 10101)
            .await
            .unwrap(),
        None
    );

    let seed = u64::MAX - 9;
    let record = dungeons::prepare_dungeon_record(&pool, 9, 7, seed, &Default::default(), &[])
        .await
        .unwrap();
    let mut tx = pool.begin().await.unwrap();
    assert!(
        dungeons::save_prepared_dungeon_record_if_faster_in_transaction(
            &mut tx, 9, 10101, 5, &record,
        )
        .await
        .unwrap()
    );
    tx.commit().await.unwrap();
    assert_eq!(
        dungeons::load_dungeon_record_seed(&pool, 9, 10101)
            .await
            .unwrap(),
        Some(seed)
    );
}

async fn battleless_pool(user_id: i64) -> SqlitePool {
    let data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data/excel2json");
    let _ = config::init(data_dir.to_str().unwrap());
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    database::run_migrations(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO users (id, username, created_at, updated_at)
             VALUES (?, 'battleless', 0, 0)",
    )
    .bind(user_id)
    .execute(&pool)
    .await
    .unwrap();
    database::db::starter_data::load_all_starter_data(&pool, user_id)
        .await
        .unwrap();
    sqlx::query("UPDATE currencies SET quantity = 100 WHERE user_id = ? AND currency_id = 4")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();
    pool
}

async fn currency_4(pool: &SqlitePool, user_id: i64) -> i32 {
    sqlx::query_scalar("SELECT quantity FROM currencies WHERE user_id = ? AND currency_id = 4")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn item_count(pool: &SqlitePool, user_id: i64, item_id: i32) -> i32 {
    sqlx::query_scalar(
        "SELECT COALESCE(MAX(quantity), 0) FROM items WHERE user_id = ? AND item_id = ?",
    )
    .bind(user_id)
    .bind(item_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn pending_row(pool: &SqlitePool, user_id: i64) -> Option<(i32, i32, i32)> {
    sqlx::query_as(
        "SELECT chapter_id, episode_id, multiplier FROM user_pending_battleless WHERE user_id = ?",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn battleless_start_consumes_cost_without_completion() {
    let pool = battleless_pool(24).await;

    match begin_battleless(&pool, 24, 10000, 1000002, 1)
        .await
        .unwrap()
    {
        BattlelessStart::Started { cost, refund } => {
            assert_eq!(cost.currency_ids, vec![(4, -6)]);
            assert!(refund.is_none());
        }
        BattlelessStart::Resumed => panic!("expected a fresh battleless start"),
    }

    assert_eq!(currency_4(&pool, 24).await, 94);
    assert_eq!(pending_row(&pool, 24).await, Some((10000, 1000002, 1)));
    assert_eq!(dungeons::episode_star(&pool, 24, 1000002).await.unwrap(), 0);
}

#[tokio::test]
async fn battleless_end_settles_completion_and_rewards() {
    let pool = battleless_pool(25).await;
    begin_battleless(&pool, 25, 10000, 1000002, 1)
        .await
        .unwrap();

    match end_battleless(&pool, 25, false).await.unwrap().unwrap() {
        BattlelessEnd::Completed {
            chapter_id,
            episode_id,
            ..
        } => {
            assert_eq!(chapter_id, 10000);
            assert_eq!(episode_id, 1000002);
        }
        BattlelessEnd::Refunded(_) => panic!("expected completion"),
    }

    assert_eq!(dungeons::episode_star(&pool, 25, 1000002).await.unwrap(), 1);
    assert_eq!(pending_row(&pool, 25).await, None);
    assert!(end_battleless(&pool, 25, false).await.unwrap().is_none());
    assert_eq!(currency_4(&pool, 25).await, 94);
}

#[tokio::test]
async fn battleless_start_defers_first_clear_reward() {
    let pool = battleless_pool(26).await;

    begin_battleless(&pool, 26, 102, 10215, 1).await.unwrap();
    assert_eq!(item_count(&pool, 26, 980025).await, 0);
    assert_eq!(dungeons::episode_star(&pool, 26, 10215).await.unwrap(), 0);

    end_battleless(&pool, 26, false).await.unwrap().unwrap();
    assert_eq!(item_count(&pool, 26, 980025).await, 1);
    assert_eq!(dungeons::episode_star(&pool, 26, 10215).await.unwrap(), 1);
}

#[tokio::test]
async fn battleless_abort_refunds_cost() {
    let pool = battleless_pool(27).await;
    begin_battleless(&pool, 27, 10000, 1000002, 1)
        .await
        .unwrap();
    assert_eq!(currency_4(&pool, 27).await, 94);

    match end_battleless(&pool, 27, true).await.unwrap().unwrap() {
        BattlelessEnd::Refunded(refund) => {
            assert_eq!(refund.rewards.currency_ids, vec![(4, 6)]);
        }
        BattlelessEnd::Completed { .. } => panic!("expected refund"),
    }

    assert_eq!(currency_4(&pool, 27).await, 100);
    assert_eq!(dungeons::episode_star(&pool, 27, 1000002).await.unwrap(), 0);
    assert_eq!(pending_row(&pool, 27).await, None);
}

#[tokio::test]
async fn battleless_start_replaces_pending_and_refunds_previous_cost() {
    let pool = battleless_pool(28).await;
    begin_battleless(&pool, 28, 10000, 1000002, 1)
        .await
        .unwrap();
    assert_eq!(currency_4(&pool, 28).await, 94);

    match begin_battleless(&pool, 28, 10000, 1000003, 1)
        .await
        .unwrap()
    {
        BattlelessStart::Started { cost, refund } => {
            assert_eq!(cost.currency_ids, vec![(4, -6)]);
            let refund = refund.expect("previous pending cost must be refunded");
            assert_eq!(refund.currency_ids, vec![(4, 6)]);
        }
        BattlelessStart::Resumed => panic!("expected a new battleless start"),
    }

    assert_eq!(currency_4(&pool, 28).await, 94);
    assert_eq!(pending_row(&pool, 28).await, Some((10000, 1000003, 1)));

    match end_battleless(&pool, 28, false).await.unwrap().unwrap() {
        BattlelessEnd::Completed { episode_id, .. } => assert_eq!(episode_id, 1000003),
        BattlelessEnd::Refunded(_) => panic!("expected completion"),
    }
    assert_eq!(dungeons::episode_star(&pool, 28, 1000003).await.unwrap(), 1);
    assert_eq!(pending_row(&pool, 28).await, None);
}

#[tokio::test]
async fn refund_commits_with_active_fight_finalization() {
    let data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data/excel2json");
    let _ = config::init(data_dir.to_str().unwrap());
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    database::run_migrations(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO users (id, username, created_at, updated_at)
             VALUES (25, 'refund', 0, 0)",
    )
    .execute(&pool)
    .await
    .unwrap();
    database::db::starter_data::load_all_starter_data(&pool, 25)
        .await
        .unwrap();
    let episode = configs::get().episode.get(10101).unwrap();
    let fight_id = battle_db::create_fight_instance(
        &pool,
        battle_db::NewFightInstance {
            user_id: 25,
            episode_id: episode.id,
            battle_id: episode.battle_id,
            multiplication: 2,
            entry_cost: "{}",
            checkpoint: "{}",
            created_at: 0,
        },
    )
    .await
    .unwrap();
    let currency_before: i32 = sqlx::query_scalar(
        "SELECT quantity FROM currencies WHERE user_id = 25 AND currency_id = 4",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let stored = battle_db::load_active_fight(&pool, 25)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.multiplication, 2);
    let active = ActiveBattle {
        fight_id: Some(fight_id + 1),
        episode_id: episode.id,
        multiplication: Some(stored.multiplication),
        ..Default::default()
    };

    assert!(settle_refund(&pool, 25, &active, false).await.is_err());
    let currency_after_failure: i32 = sqlx::query_scalar(
        "SELECT quantity FROM currencies WHERE user_id = 25 AND currency_id = 4",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(currency_after_failure, currency_before);
    assert!(
        battle_db::load_active_fight(&pool, 25)
            .await
            .unwrap()
            .is_some()
    );

    settle_checkpoint_refund(&pool, 25, fight_id, reward::parse("2#4#16"))
        .await
        .unwrap();
    let currency_after_commit: i32 = sqlx::query_scalar(
        "SELECT quantity FROM currencies WHERE user_id = 25 AND currency_id = 4",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(currency_after_commit, currency_before + 16);
    assert!(
        battle_db::load_active_fight(&pool, 25)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn point_reward_claim_uses_shared_progress_and_is_idempotent() {
    let data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data/excel2json");
    let _ = config::init(data_dir.to_str().unwrap());
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    database::run_migrations(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO users (id, username, created_at, updated_at)
         VALUES (26, 'point-reward', 0, 0)",
    )
    .execute(&pool)
    .await
    .unwrap();
    database::db::starter_data::load_all_starter_data(&pool, 26)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE user_dungeon_reward_points
         SET reward_point = 10
         WHERE user_id = 26 AND chapter_id = 0",
    )
    .execute(&pool)
    .await
    .unwrap();
    let currency_before: i32 = sqlx::query_scalar(
        "SELECT quantity FROM currencies WHERE user_id = 26 AND currency_id = 2",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let claim = logic::dungeon::DungeonManager::new(26)
        .claim_point_rewards(&pool, vec![1])
        .await
        .unwrap();

    assert_eq!(claim.reply.id, vec![1]);
    let currency_after: i32 = sqlx::query_scalar(
        "SELECT quantity FROM currencies WHERE user_id = 26 AND currency_id = 2",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(currency_after, currency_before + 50);
    let points = dungeons::get_reward_points(&pool, 26).await.unwrap();
    assert_eq!(points[0].has_get_point_reward_ids, vec![1]);
    assert!(
        logic::dungeon::DungeonManager::new(26)
            .claim_point_rewards(&pool, vec![1])
            .await
            .is_err()
    );
    let currency_after_retry: i32 = sqlx::query_scalar(
        "SELECT quantity FROM currencies WHERE user_id = 26 AND currency_id = 2",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(currency_after_retry, currency_after);
}
