use crate::{
    error::AppError,
    net::{context::ConnectionContext, packet::ClientPacket},
    siege,
};
use prost::Message;
use sonettobuf::{
    AbandonSiegeBattleReply, AbandonSiegeBattleRequest, CmdId, GetSiegeBattleInfoRequest,
    StartSiegeBattleReply, StartSiegeBattleRequest,
};

pub async fn on_get_info(ctx: &mut ConnectionContext, req: ClientPacket) -> Result<(), AppError> {
    GetSiegeBattleInfoRequest::decode(&req.data[..])?;
    let reply = siege::info(ctx.state.db, ctx.player()?.id).await?;
    ctx.send_reply(CmdId::GetSiegeBattleInfoCmd, reply, 0, req.up_tag)
        .await
}

// Siege commands only sync state. The fight itself runs through
// `StartDungeonCmd`, and progress is derived from `user_dungeons.star`, so
// start/abandon intentionally just echo the current info.
pub async fn on_start(ctx: &mut ConnectionContext, req: ClientPacket) -> Result<(), AppError> {
    StartSiegeBattleRequest::decode(&req.data[..])?;
    let info = siege::info(ctx.state.db, ctx.player()?.id).await?.info;
    ctx.send_reply(
        CmdId::StartSiegeBattleCmd,
        StartSiegeBattleReply { info },
        0,
        req.up_tag,
    )
    .await
}

pub async fn on_abandon(ctx: &mut ConnectionContext, req: ClientPacket) -> Result<(), AppError> {
    AbandonSiegeBattleRequest::decode(&req.data[..])?;
    let info = siege::info(ctx.state.db, ctx.player()?.id).await?.info;
    ctx.send_reply(
        CmdId::AbandonSiegeBattleCmd,
        AbandonSiegeBattleReply { info },
        0,
        req.up_tag,
    )
    .await
}
