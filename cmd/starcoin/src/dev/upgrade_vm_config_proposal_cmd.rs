// Copyright (c) The Starcoin Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::cli_state::CliState;
use crate::dev::sign_txn_helper::get_dao_config;
use crate::view::{ExecuteResultView, TransactionOptions};
use crate::StarcoinOpt;
use anyhow::Result;
use scmd::{CommandAction, ExecContext};
use starcoin_config::BuiltinNetworkID;
use starcoin_transaction_builder::build_vm_config_upgrade_proposal;
use starcoin_vm_types::transaction::TransactionPayload;
use structopt::StructOpt;

/// Submit a VM config upgrade proposal
#[derive(Debug, StructOpt)]
#[structopt(name = "vm-config-proposal", alias = "vm_config_proposal")]
#[allow(clippy::upper_case_acronyms)]
pub struct UpgradeVMConfigProposalOpt {
    #[structopt(flatten)]
    transaction_opts: TransactionOptions,

    #[structopt(short = "n", name = "net", long = "net")]
    /// The network id for copy config
    net: BuiltinNetworkID,
}

#[allow(clippy::upper_case_acronyms)]
pub struct UpgradeVMConfigProposalCommand;

impl CommandAction for UpgradeVMConfigProposalCommand {
    type State = CliState;
    type GlobalOpt = StarcoinOpt;
    type Opt = UpgradeVMConfigProposalOpt;
    type ReturnItem = ExecuteResultView;

    fn run(
        &self,
        ctx: &ExecContext<Self::State, Self::GlobalOpt, Self::Opt>,
    ) -> Result<Self::ReturnItem> {
        let opt = ctx.opt();

        let genesis_config = opt.net.genesis_config().clone();
        let min_action_delay = get_dao_config(ctx.state())?.min_action_delay;
        let vm_config_upgrade_proposal =
            build_vm_config_upgrade_proposal(genesis_config.vm_config, min_action_delay);
        ctx.state().build_and_execute_transaction(
            opt.transaction_opts.clone(),
            TransactionPayload::ScriptFunction(vm_config_upgrade_proposal),
        )
    }
}
