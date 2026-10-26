use super::*;
use database::db::game::dungeons;
use sqlx::SqlitePool;

async fn test_pool(user_id: i64) -> SqlitePool {
    let data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data/excel2json");
    let _ = config::init(data_dir.to_str().unwrap());
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    database::run_migrations(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO users (id, username, created_at, updated_at)
         VALUES (?, ?, 0, 0)",
    )
    .bind(user_id)
    .bind(format!("fairytale-{user_id}"))
    .execute(&pool)
    .await
    .unwrap();
    database::db::starter_data::load_all_starter_data(&pool, user_id)
        .await
        .unwrap();
    pool
}

#[tokio::test]
async fn story_first_clear_completes_fairytale_in_same_transaction_and_grants_rewards() {
    let pool = test_pool(101).await;
    let episode = configs::get().episode.get(10403).unwrap();
    let fairytale = configs::get().linked_fairytale_episode(episode).unwrap();
    assert_eq!(fairytale.id, 403);

    let currency_before: i32 = sqlx::query_scalar(
        "SELECT quantity FROM currencies WHERE user_id = 101 AND currency_id = 2",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let item_before: i32 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(quantity), 0) FROM items WHERE user_id = 101 AND item_id = 110202",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let mut tx = pool.begin().await.unwrap();
    let settlement = crate::dungeon::settle_completion_in_transaction(
        &mut tx,
        101,
        episode.chapter_id,
        episode.id,
        DungeonCompletion {
            star: 2,
            total_round: 1,
            multiplier: 1,
            fight_group: None,
        },
        &Default::default(),
    )
    .await
    .unwrap();

    // Verify both level statuses are present in the settlement
    assert_eq!(
        settlement.dungeon_update.dungeon_info.as_ref().and_then(|d| d.episode_id),
        Some(10403)
    );
    assert_eq!(
        settlement.dungeon_update.dungeon_info.as_ref().and_then(|d| d.star),
        Some(2)
    );

    let ft_update = settlement
        .fairytale_dungeon_update
        .expect("fairytale_dungeon_update must be present on first clear");
    assert_eq!(
        ft_update.dungeon_info.as_ref().and_then(|d| d.episode_id),
        Some(403)
    );
    assert_eq!(
        ft_update.dungeon_info.as_ref().and_then(|d| d.star),
        Some(2)
    );

    tx.commit().await.unwrap();

    // Check database state: both story and fairytale are cleared
    let story_star = dungeons::episode_star(&pool, 101, 10403).await.unwrap();
    let fairytale_star = dungeons::episode_star(&pool, 101, 403).await.unwrap();
    assert_eq!(story_star, 2);
    assert_eq!(fairytale_star, 2);

    // Check rewards: story advanced bonus (60) + fairytale first bonus (20) = 80 Clear Drops
    let currency_after: i32 = sqlx::query_scalar(
        "SELECT quantity FROM currencies WHERE user_id = 101 AND currency_id = 2",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(currency_after, currency_before + 80);

    // Check item reward from fairytale: 3 of item 110202
    let item_after: i32 = sqlx::query_scalar(
        "SELECT quantity FROM items WHERE user_id = 101 AND item_id = 110202",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(item_after, item_before + 3);
}

#[tokio::test]
async fn story_repeat_clear_does_not_re_reward_or_re_clear_fairytale() {
    let pool = test_pool(102).await;
    let episode = configs::get().episode.get(10403).unwrap();

    // First clear
    let mut tx1 = pool.begin().await.unwrap();
    crate::dungeon::settle_completion_in_transaction(
        &mut tx1,
        102,
        episode.chapter_id,
        episode.id,
        DungeonCompletion {
            star: 2,
            total_round: 1,
            multiplier: 1,
            fight_group: None,
        },
        &Default::default(),
    )
    .await
    .unwrap();
    tx1.commit().await.unwrap();

    let currency_after_first: i32 = sqlx::query_scalar(
        "SELECT quantity FROM currencies WHERE user_id = 102 AND currency_id = 2",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let item_after_first: i32 = sqlx::query_scalar(
        "SELECT quantity FROM items WHERE user_id = 102 AND item_id = 110202",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // Repeat clear
    let mut tx2 = pool.begin().await.unwrap();
    let settlement2 = crate::dungeon::settle_completion_in_transaction(
        &mut tx2,
        102,
        episode.chapter_id,
        episode.id,
        DungeonCompletion {
            star: 2,
            total_round: 1,
            multiplier: 1,
            fight_group: None,
        },
        &Default::default(),
    )
    .await
    .unwrap();

    // Fairytale update must NOT be present on repeat clear
    assert!(settlement2.fairytale_dungeon_update.is_none());
    tx2.commit().await.unwrap();

    // Currencies and items must not increase on repeat clear
    let currency_after_repeat: i32 = sqlx::query_scalar(
        "SELECT quantity FROM currencies WHERE user_id = 102 AND currency_id = 2",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let item_after_repeat: i32 = sqlx::query_scalar(
        "SELECT quantity FROM items WHERE user_id = 102 AND item_id = 110202",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(currency_after_repeat, currency_after_first);
    assert_eq!(item_after_repeat, item_after_first);
}

#[tokio::test]
async fn story_first_clear_when_fairytale_already_cleared_does_not_duplicate_fairytale_rewards() {
    let pool = test_pool(103).await;
    let episode = configs::get().episode.get(10403).unwrap();
    let fairytale = configs::get().linked_fairytale_episode(episode).unwrap();

    // Pre-clear fairytale episode directly
    let mut tx_ft = pool.begin().await.unwrap();
    crate::dungeon::settle_completion_in_transaction(
        &mut tx_ft,
        103,
        fairytale.chapter_id,
        fairytale.id,
        DungeonCompletion {
            star: 2,
            total_round: 1,
            multiplier: 1,
            fight_group: None,
        },
        &Default::default(),
    )
    .await
    .unwrap();
    tx_ft.commit().await.unwrap();

    assert_eq!(dungeons::episode_star(&pool, 103, 403).await.unwrap(), 2);
    assert_eq!(dungeons::episode_star(&pool, 103, 10403).await.unwrap(), 0);

    let currency_before_story: i32 = sqlx::query_scalar(
        "SELECT quantity FROM currencies WHERE user_id = 103 AND currency_id = 2",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let item_before_story: i32 = sqlx::query_scalar(
        "SELECT quantity FROM items WHERE user_id = 103 AND item_id = 110202",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // Now clear story for the first time
    let mut tx_story = pool.begin().await.unwrap();
    let settlement = crate::dungeon::settle_completion_in_transaction(
        &mut tx_story,
        103,
        episode.chapter_id,
        episode.id,
        DungeonCompletion {
            star: 2,
            total_round: 1,
            multiplier: 1,
            fight_group: None,
        },
        &Default::default(),
    )
    .await
    .unwrap();

    // Since fairytale was already cleared, fairytale_dungeon_update is None
    assert!(settlement.fairytale_dungeon_update.is_none());
    tx_story.commit().await.unwrap();

    // Only story advanced bonus (60) should be granted, NOT fairytale first bonus (20)
    let currency_after_story: i32 = sqlx::query_scalar(
        "SELECT quantity FROM currencies WHERE user_id = 103 AND currency_id = 2",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(currency_after_story, currency_before_story + 60);

    // Fairytale item (110202) must not increase again
    let item_after_story: i32 = sqlx::query_scalar(
        "SELECT quantity FROM items WHERE user_id = 103 AND item_id = 110202",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(item_after_story, item_before_story);
}

#[tokio::test]
async fn historical_compensation_clears_fairytale_and_is_idempotent() {
    let pool = test_pool(104).await;

    // Simulate an existing player who cleared story 10403 in the past without fairytale 403
    sqlx::query(
        "INSERT INTO user_dungeons
         (user_id, chapter_id, episode_id, star, challenge_count, has_record,
          left_return_all_num, today_pass_num, today_total_num, created_at, updated_at)
         VALUES (104, 104, 10403, 2, 1, 0, 1, 1, 0, 0, 0)",
    )
    .execute(&pool)
    .await
    .unwrap();

    assert_eq!(dungeons::episode_star(&pool, 104, 10403).await.unwrap(), 2);
    assert_eq!(dungeons::episode_star(&pool, 104, 403).await.unwrap(), 0);

    let currency_before: i32 = sqlx::query_scalar(
        "SELECT quantity FROM currencies WHERE user_id = 104 AND currency_id = 2",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let item_before: i32 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(quantity), 0) FROM items WHERE user_id = 104 AND item_id = 110202",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // Run historical compensation
    let compensation = info::compensate_fairytale_clears(&pool, 104)
        .await
        .unwrap();

    assert_eq!(compensation.compensated_episodes, vec![403]);
    assert_eq!(compensation.dungeon_updates.len(), 1);
    assert_eq!(
        compensation.dungeon_updates[0]
            .dungeon_info
            .as_ref()
            .and_then(|d| d.episode_id),
        Some(403)
    );

    // Database should now have fairytale 403 cleared
    assert_eq!(dungeons::episode_star(&pool, 104, 403).await.unwrap(), 2);

    // Rewards from fairytale first bonus must be granted
    let currency_after: i32 = sqlx::query_scalar(
        "SELECT quantity FROM currencies WHERE user_id = 104 AND currency_id = 2",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(currency_after, currency_before + 20);

    let item_after: i32 = sqlx::query_scalar(
        "SELECT quantity FROM items WHERE user_id = 104 AND item_id = 110202",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(item_after, item_before + 3);

    // Second run must be completely idempotent
    let compensation2 = info::compensate_fairytale_clears(&pool, 104)
        .await
        .unwrap();
    assert!(compensation2.compensated_episodes.is_empty());
    assert!(compensation2.dungeon_updates.is_empty());

    let currency_after2: i32 = sqlx::query_scalar(
        "SELECT quantity FROM currencies WHERE user_id = 104 AND currency_id = 2",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(currency_after2, currency_after);

    let item_after2: i32 = sqlx::query_scalar(
        "SELECT quantity FROM items WHERE user_id = 104 AND item_id = 110202",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(item_after2, item_after);
}

#[tokio::test]
async fn settlement_failure_rolls_back_both_story_and_fairytale() {
    let pool = test_pool(105).await;
    let episode = configs::get().episode.get(10403).unwrap();

    let currency_before: i32 = sqlx::query_scalar(
        "SELECT quantity FROM currencies WHERE user_id = 105 AND currency_id = 2",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let item_before: i32 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(quantity), 0) FROM items WHERE user_id = 105 AND item_id = 110202",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // Start transaction, settle, and deliberately drop without commit (simulating rollback on error)
    {
        let mut tx = pool.begin().await.unwrap();
        let settlement = crate::dungeon::settle_completion_in_transaction(
            &mut tx,
            105,
            episode.chapter_id,
            episode.id,
            DungeonCompletion {
                star: 2,
                total_round: 1,
                multiplier: 1,
                fight_group: None,
            },
            &Default::default(),
        )
        .await
        .unwrap();

        assert!(settlement.fairytale_dungeon_update.is_some());
        // Deliberately rollback
        tx.rollback().await.unwrap();
    }

    // Verify neither story nor fairytale was saved
    assert_eq!(dungeons::episode_star(&pool, 105, 10403).await.unwrap(), 0);
    assert_eq!(dungeons::episode_star(&pool, 105, 403).await.unwrap(), 0);

    // Verify rewards were not kept
    let currency_after: i32 = sqlx::query_scalar(
        "SELECT quantity FROM currencies WHERE user_id = 105 AND currency_id = 2",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let item_after: i32 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(quantity), 0) FROM items WHERE user_id = 105 AND item_id = 110202",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(currency_after, currency_before);
    assert_eq!(item_after, item_before);
}

#[tokio::test]
async fn hard_mode_peril_settlement_does_not_affect_fairytale() {
    let pool = test_pool(106).await;
    let episode = configs::get().episode.get(20401).unwrap();
    assert_eq!(configs::get().chapter.get(episode.chapter_id).unwrap().r#type, 2);

    let mut tx = pool.begin().await.unwrap();
    let settlement = crate::dungeon::settle_completion_in_transaction(
        &mut tx,
        106,
        episode.chapter_id,
        episode.id,
        DungeonCompletion {
            star: 2,
            total_round: 1,
            multiplier: 1,
            fight_group: None,
        },
        &Default::default(),
    )
    .await
    .unwrap();

    // Fairytale must not be affected by hard mode clear
    assert!(settlement.fairytale_dungeon_update.is_none());
    tx.commit().await.unwrap();

    assert_eq!(dungeons::episode_star(&pool, 106, 20401).await.unwrap(), 2);
    assert_eq!(dungeons::episode_star(&pool, 106, 403).await.unwrap(), 0);
}
