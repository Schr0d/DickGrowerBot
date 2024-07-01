use teloxide::prelude::{ChatId, UserId};
use testcontainers::clients;
use crate::repo;
use crate::repo::{ChatIdKind, ChatIdPartiality};
use crate::repo::test::{CHAT_ID, start_postgres, UID};
use crate::repo::test::dicks::create_user;

#[tokio::test]
async fn test_all() {
    let docker = clients::Cli::default();
    let (_container, db) = start_postgres(&docker).await;
    let personal_stats = repo::PersonalStatsRepo::new(db.clone());
    let dicks = repo::Dicks::new(db.clone(), Default::default());
    
    let chat_id_1 = ChatIdKind::ID(ChatId(CHAT_ID));
    let chat_id_2 = ChatIdKind::ID(ChatId(CHAT_ID + 1));
    let uid = UserId(UID as u64);
    create_user(&db).await;
    
    let stats = personal_stats.get(uid).await
        .expect("couldn't fetch the empty stats");
    assert_eq!(stats.chats, 0);
    assert_eq!(stats.max_length, 0);
    assert_eq!(stats.total_length, 0);
    
    dicks.create_or_grow(uid, &ChatIdPartiality::Specific(chat_id_1), 10).await
        .expect("couldn't grow the dick in the first chat");
    dicks.create_or_grow(uid, &ChatIdPartiality::Specific(chat_id_2), 20).await
        .expect("couldn't grow the dick in the second chat");

    let stats = personal_stats.get(uid).await
        .expect("couldn't fetch the non-null stats");
    assert_eq!(stats.chats, 2);
    assert_eq!(stats.max_length, 20);
    assert_eq!(stats.total_length, 30);
}
