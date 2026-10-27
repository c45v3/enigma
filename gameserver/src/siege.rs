use config::configs;
use database::db::game::dungeons;
use sonettobuf::{GetSiegeBattleInfoReply, SiegeBattleInfo};
use sqlx::SqlitePool;

use crate::error::AppError;

pub async fn info(db: &SqlitePool, player_id: i64) -> Result<GetSiegeBattleInfoReply, AppError> {
    let mut pass_challenge_ids = Vec::new();
    for challenge in configs::get().siege_battle.iter() {
        if dungeons::episode_star(db, player_id, challenge.episode_id).await? > 0 {
            pass_challenge_ids.push(challenge.id);
        }
    }

    Ok(GetSiegeBattleInfoReply {
        info: Some(SiegeBattleInfo {
            open_challenge: Some(true),
            pass_challenge_ids,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn info_reports_completed_challenge_ids() {
        let data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("data/excel2json");
        let _ = config::init(data_dir.to_str().unwrap());
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        database::run_migrations(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO users (id, username, created_at, updated_at)
             VALUES (?, 'siege-info', 0, 0)",
        )
        .bind(7)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO user_dungeons
             (user_id, chapter_id, episode_id, star, challenge_count, has_record,
              left_return_all_num, today_pass_num, today_total_num, created_at, updated_at)
             VALUES (?, ?, ?, ?, 0, 0, 1, 0, 0, 0, 0)",
        )
        .bind(7)
        .bind(19101)
        .bind(1910103)
        .bind(2)
        .execute(&pool)
        .await
        .unwrap();

        let reply = info(&pool, 7).await.unwrap();
        let info = reply.info.unwrap();
        assert!(info.open_challenge.unwrap());
        assert_eq!(info.pass_challenge_ids, vec![3]);
    }
}
