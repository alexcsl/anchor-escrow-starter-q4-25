use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use anchor_spl::{
    associated_token::spl_associated_token_account,
    token::spl_token,
};
use litesvm::LiteSVM;
use solana_sdk::{
    account::Account,
    instruction::Instruction,
    program_option::COption,
    program_pack::Pack,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_program,
    transaction::Transaction,
};
use spl_token::state::{Account as TokenAccount, AccountState, Mint};

fn send(svm: &mut LiteSVM, payer: &Keypair, ix: Instruction) {
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[payer],
        svm.latest_blockhash(),
    );

    svm.send_transaction(tx).unwrap();
}

struct Fixture {
    svm: LiteSVM,
    maker: Keypair,
    mint_a: Pubkey,
    mint_b: Pubkey,
    maker_ata_a: Pubkey,
    escrow: Pubkey,
    vault: Pubkey,
}

fn setup() -> Fixture {
    let mut svm = LiteSVM::new();
    let program_id = anchor_escrow_q4_25::id();
    let maker = Keypair::new();

    let program_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/deploy/anchor_escrow_q4_25.so");

    svm.add_program_from_file(program_id, program_path)
        .unwrap();

    svm.airdrop(&maker.pubkey(), 2_000_000_000).unwrap();

    let mint_a = Pubkey::new_unique();
    let mint_b = Pubkey::new_unique();

    for mint in [mint_a, mint_b] {
        let mut data: Vec<u8> = vec![0; Mint::LEN];

        Mint::pack(
            Mint {
                mint_authority: COption::Some(maker.pubkey()),
                supply: if mint == mint_a { 100 } else { 0 },
                decimals: 0,
                is_initialized: true,
                freeze_authority: COption::None,
            },
            &mut data,
        )
        .unwrap();

        svm.set_account(
            mint,
            Account {
                lamports: svm.minimum_balance_for_rent_exemption(data.len()),
                data: data.to_vec(),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    }

    let maker_ata_a =
        spl_associated_token_account::get_associated_token_address(
            &maker.pubkey(),
            &mint_a,
        );

    let mut data: Vec<u8> = vec![0; TokenAccount::LEN];

    TokenAccount::pack(
        TokenAccount {
            mint: mint_a,
            owner: maker.pubkey(),
            amount: 100,
            delegate: COption::None,
            state: AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 0,
            close_authority: COption::None,
        },
        &mut data,
    )
    .unwrap();

    svm.set_account(
        maker_ata_a,
        Account {
            lamports: svm.minimum_balance_for_rent_exemption(data.len()),
            data,
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    let seed = 1u64;

    let (escrow, _) = Pubkey::find_program_address(
        &[b"escrow", maker.pubkey().as_ref(), &seed.to_le_bytes()],
        &program_id,
    );

    let vault =
        spl_associated_token_account::get_associated_token_address(
            &escrow,
            &mint_a,
        );

    send(
        &mut svm,
        &maker,
        Instruction {
            program_id,
            accounts: anchor_escrow_q4_25::accounts::Make {
                maker: maker.pubkey(),
                mint_a,
                mint_b,
                maker_ata_a,
                escrow,
                vault,
                associated_token_program: spl_associated_token_account::ID,
                token_program: spl_token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: anchor_escrow_q4_25::instruction::Make {
                seed,
                deposit: 100,
                receive: 200,
            }
            .data(),
        },
    );

    Fixture {
        svm,
        maker,
        mint_a,
        mint_b,
        maker_ata_a,
        escrow,
        vault,
    }
}

#[test]
fn make() {
    let Fixture {
        svm, maker, mint_a, mint_b, maker_ata_a, escrow, vault,
    } = setup();

    let account = svm.get_account(&escrow).unwrap();
    let state =
        anchor_escrow_q4_25::Escrow::try_deserialize(&mut account.data.as_slice())
            .unwrap();

    assert_eq!(state.maker, maker.pubkey());
    assert_eq!(state.mint_a, mint_a);
    assert_eq!(state.mint_b, mint_b);
    assert_eq!(state.receive, 200);

    let account = svm.get_account(&vault).unwrap();
    let tokens = TokenAccount::unpack(&account.data).unwrap();

    assert_eq!(tokens.amount, 100);

    let account = svm.get_account(&maker_ata_a).unwrap();
    let tokens = TokenAccount::unpack(&account.data).unwrap();

    assert_eq!(tokens.amount, 0);
}

#[test]
fn update() {
    let Fixture { mut svm, maker, escrow, vault, .. } = setup();

    send(
        &mut svm,
        &maker,
        Instruction {
            program_id: anchor_escrow_q4_25::id(),
            accounts: anchor_escrow_q4_25::accounts::Update {
                maker: maker.pubkey(),
                escrow,
            }
            .to_account_metas(None),
            data: anchor_escrow_q4_25::instruction::Update { receive: 150 }.data(),
        },
    );

    let account = svm.get_account(&escrow).unwrap();
    let state =
        anchor_escrow_q4_25::Escrow::try_deserialize(&mut account.data.as_slice())
            .unwrap();

    assert_eq!(state.receive, 150);

    let account = svm.get_account(&vault).unwrap();
    let tokens = TokenAccount::unpack(&account.data).unwrap();

    assert_eq!(tokens.amount, 100);
}

#[test]
fn refund() {
    let Fixture {
        mut svm, maker, mint_a, maker_ata_a, escrow, vault, ..
    } = setup();

    send(
        &mut svm,
        &maker,
        Instruction {
            program_id: anchor_escrow_q4_25::id(),
            accounts: anchor_escrow_q4_25::accounts::Refund {
                maker: maker.pubkey(),
                mint_a,
                maker_ata_a,
                escrow,
                vault,
                associated_token_program: spl_associated_token_account::ID,
                token_program: spl_token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: anchor_escrow_q4_25::instruction::Refund {}.data(),
        },
    );

    assert!(svm.get_account(&escrow).is_none());
    assert!(svm.get_account(&vault).is_none());

    let account = svm.get_account(&maker_ata_a).unwrap();
    let tokens = TokenAccount::unpack(&account.data).unwrap();

    assert_eq!(tokens.amount, 100);
}

#[test]
fn take() {
    let Fixture {
        mut svm, maker, mint_a, mint_b, maker_ata_a, escrow, vault,
    } = setup();
    let taker = Keypair::new();

    svm.airdrop(&taker.pubkey(), 2_000_000_000).unwrap();

    let taker_ata_b =
        spl_associated_token_account::get_associated_token_address(
            &taker.pubkey(),
            &mint_b,
        );

    send(
        &mut svm,
        &maker,
        spl_associated_token_account::instruction::create_associated_token_account(
            &maker.pubkey(),
            &taker.pubkey(),
            &mint_b,
            &spl_token::ID,
        ),
    );

    send(
        &mut svm,
        &maker,
        spl_token::instruction::mint_to(
            &spl_token::ID,
            &mint_b,
            &taker_ata_b,
            &maker.pubkey(),
            &[],
            200,
        )
        .unwrap(),
    );

    send(
        &mut svm,
        &maker,
        Instruction {
            program_id: anchor_escrow_q4_25::id(),
            accounts: anchor_escrow_q4_25::accounts::Update {
                maker: maker.pubkey(),
                escrow,
            }
            .to_account_metas(None),
            data: anchor_escrow_q4_25::instruction::Update { receive: 150 }.data(),
        },
    );

    let taker_ata_a =
        spl_associated_token_account::get_associated_token_address(
            &taker.pubkey(),
            &mint_a,
        );
    let maker_ata_b =
        spl_associated_token_account::get_associated_token_address(
            &maker.pubkey(),
            &mint_b,
        );

    send(
        &mut svm,
        &taker,
        Instruction {
            program_id: anchor_escrow_q4_25::id(),
            accounts: anchor_escrow_q4_25::accounts::Take {
                taker: taker.pubkey(),
                maker: maker.pubkey(),
                mint_a,
                mint_b,
                taker_ata_a,
                taker_ata_b,
                maker_ata_b,
                escrow,
                vault,
                associated_token_program: spl_associated_token_account::ID,
                token_program: spl_token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: anchor_escrow_q4_25::instruction::Take {}.data(),
        },
    );

    assert!(svm.get_account(&escrow).is_none());
    assert!(svm.get_account(&vault).is_none());

    for (address, amount) in [
        (maker_ata_a, 0),
        (taker_ata_a, 100),
        (maker_ata_b, 150),
        (taker_ata_b, 50),
    ] {
        let account = svm.get_account(&address).unwrap();
        let tokens = TokenAccount::unpack(&account.data).unwrap();

        assert_eq!(tokens.amount, amount);
    }
}
