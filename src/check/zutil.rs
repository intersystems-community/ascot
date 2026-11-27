//! The `$ZUTIL` functions IRIS superseded or deprecated, and what replaces them.
//!
//! Transcribed from the InterSystems Language Server's `zutilFunctions` table, which follows
//! "Replacements for ObjectScript $ZUTIL Functions" (the Caché 2018.1 ObjectScript Reference, the
//! last to document `$ZUTIL`) and keeps it current. The advice for a function the Language Server
//! can't replace mechanically is that page's.

/// A legacy `$ZU` function: the leading literal arguments that identify it, whether more arguments
/// must follow them (`Some(true)`), must not (`Some(false)`) or may (`None`), and what to use
/// instead -- `None` for a deprecated function with no replacement.
pub(crate) struct Legacy {
    pub args: &'static [u32],
    pub more: Option<bool>,
    pub replacement: Option<&'static str>,
}

#[allow(non_snake_case)]
const fn L(args: &'static [u32], replacement: Option<&'static str>) -> Legacy {
    Legacy {
        args,
        more: None,
        replacement,
    }
}

pub(crate) const LEGACY: &[Legacy] = &[
    L(&[4], Some("$SYSTEM.Process.Terminate()")),
    L(&[5], Some("$NAMESPACE")),
    L(
        &[9],
        Some("##class(%Device).Broadcast() or ##class(%SYS.System).WriteToConsoleLog()"),
    ),
    L(&[12], Some("##class(%File).NormalizeDirectory()")),
    L(&[15], Some("##class(%File).NormalizeFilename()")),
    L(&[18], Some("$SYSTEM.Process.Undefined()")),
    L(&[20], Some("$SYSTEM.Process.UserRoutinePath()")),
    L(&[21], Some("$SYSTEM.Process.PrivateGlobalLocation()")),
    L(&[21, 0], Some("$SYSTEM.Process.PrivateGlobalLocation()")),
    L(&[21, 1], Some("$SYSTEM.Process.KillAllPrivateGlobals()")),
    L(&[21, 2], Some("$SYSTEM.Process.KillAllPrivateGlobals()")),
    L(&[22, 0], Some("##class(%Device).SetFFBS()")),
    L(&[28], Some("$SYSTEM.Util.Collation()")),
    L(&[39], Some("$SYSTEM.Process.SysRoutinePath()")),
    L(&[49], Some("the SYS.Database class properties")),
    Legacy {
        more: Some(false),
        ..L(&[53], Some("$SYSTEM.INetInfo.TCPName()"))
    },
    Legacy {
        more: Some(true),
        ..L(&[53], Some("$SYSTEM.INetInfo.TCPStats()"))
    },
    L(&[55], Some("$SYSTEM.Process.LanguageMode()")),
    L(&[56, 2], Some("$SYSTEM.Process.ErrorLine()")),
    L(&[56, 6], Some("$SYSTEM.Process.OSError()")),
    L(&[62], Some("##class(%Routine).CheckSyntax()")),
    L(&[67, 0], Some("$SYSTEM.Process.IsGhost()")),
    L(&[67, 1], None),
    L(&[67, 4], Some("$SYSTEM.Process.State()")),
    L(&[67, 5], Some("$SYSTEM.Process.Routine()")),
    L(&[67, 6], Some("$SYSTEM.Process.NameSpace()")),
    L(&[67, 7], Some("$SYSTEM.Process.CurrentDevice()")),
    L(&[67, 8], Some("$SYSTEM.Process.LinesExecuted()")),
    L(&[67, 9], Some("$SYSTEM.Process.GlobalReferences()")),
    L(&[67, 10], Some("$SYSTEM.Process.JobType()")),
    L(&[67, 11], Some("$SYSTEM.Process.UserName()")),
    L(&[67, 12], Some("$SYSTEM.Process.ClientNodeName()")),
    L(&[67, 13], Some("$SYSTEM.Process.ClientExecutableName()")),
    L(&[67, 14], Some("$SYSTEM.Process.CSPSessionID()")),
    L(&[67, 15], Some("$SYSTEM.Process.ClientIPAddress()")),
    L(&[68, 1], Some("$SYSTEM.Process.NullSubscripts()")),
    L(&[68, 2], Some("$SYSTEM.Process.OpenMode()")),
    L(&[68, 3], Some("$SYSTEM.Process.FileMode()")),
    L(&[68, 5], Some("$SYSTEM.Process.BreakMode()")),
    L(&[68, 6], None),
    L(&[68, 7], Some("$SYSTEM.Process.RefInKind()")),
    L(&[68, 11], Some("$SYSTEM.Process.LineRecall()")),
    L(&[68, 15], Some("$SYSTEM.Process.DisconnectErr()")),
    L(&[68, 21], Some("$SYSTEM.Process.SynchCommit()")),
    L(&[68, 22], Some("$SYSTEM.Process.DX()")),
    L(&[68, 25], Some("$SYSTEM.Process.BatchFlag()")),
    L(&[68, 26], Some("$SYSTEM.Process.TerminalPrompt()")),
    L(&[68, 27], None),
    L(&[68, 28], Some("$SYSTEM.Process.GlobalKillDisabled()")),
    L(&[68, 30], Some("$SYSTEM.Process.PopError()")),
    L(&[68, 32], Some("$SYSTEM.Process.ZDateNull()")),
    L(&[68, 34], Some("$SYSTEM.Process.AsynchError()")),
    L(&[68, 39], None),
    L(&[68, 40], Some("$SYSTEM.Process.SetZEOF()")),
    L(&[68, 42], Some("$SYSTEM.Process.NodeNameInPid()")),
    L(&[68, 43], Some("$SYSTEM.Process.OldZU5()")),
    L(&[68, 45], Some("$SYSTEM.Process.TruncateOverflow()")),
    L(&[68, 51], Some("$SYSTEM.Process.SwitchOSdir()")),
    L(&[68, 55], None),
    L(&[68, 60], Some("$SYSTEM.Process.AsyncDisconnectErr()")),
    L(&[68, 63], Some("$SYSTEM.Process.ScientificNotation()")),
    L(&[68, 66], Some("$SYSTEM.Process.TelnetNUL()")),
    L(&[68, 67], Some("$SYSTEM.Process.ExceptionLog()")),
    L(&[68, 70], Some("$SYSTEM.Process.IEEEError()")),
    L(&[68, 71], Some("$SYSTEM.Process.IPv6Format()")),
    L(&[68, 72], Some("$SYSTEM.Process.MVUndefined()")),
    L(
        &[69, 0],
        Some("the Undefined property of Config.Miscellaneous"),
    ),
    L(
        &[69, 1],
        Some("the NullSubscripts property of Config.Miscellaneous"),
    ),
    L(
        &[69, 2],
        Some("the OpenMode property of Config.Miscellaneous"),
    ),
    L(
        &[69, 3],
        Some("the FileMode property of Config.Miscellaneous"),
    ),
    L(
        &[69, 5],
        Some("the BreakMode property of Config.Miscellaneous"),
    ),
    L(&[69, 6], None),
    L(
        &[69, 7],
        Some("the RefInKind property of Config.Miscellaneous"),
    ),
    L(
        &[69, 8],
        Some("the ZaMode property of Config.Miscellaneous"),
    ),
    L(
        &[69, 10],
        Some("the FreezeOnError property of Config.Journal"),
    ),
    L(
        &[69, 11],
        Some("the LineRecall property of Config.Miscellaneous"),
    ),
    L(&[69, 13], None),
    L(&[69, 14], None),
    L(
        &[69, 15],
        Some("the DisconnectErr property of Config.Miscellaneous"),
    ),
    L(&[69, 19], None),
    L(&[69, 20], None),
    L(
        &[69, 21],
        Some("the SynchCommit property of Config.Miscellaneous"),
    ),
    L(&[69, 22], Some("the DX property of Config.Miscellaneous")),
    L(
        &[69, 26],
        Some("the TerminalPrompt property of Config.Startup"),
    ),
    L(&[69, 27], None),
    L(&[69, 28], None),
    L(
        &[69, 30],
        Some("the PopError property of Config.Miscellaneous"),
    ),
    L(&[69, 31], None),
    L(
        &[69, 32],
        Some("the ZDateNull property of Config.Miscellaneous"),
    ),
    L(
        &[69, 34],
        Some("the AsynchError property of Config.Miscellaneous"),
    ),
    L(&[69, 35], None),
    L(
        &[69, 37],
        Some("the PhysicalCursor property of Config.NLS.Locales"),
    ),
    L(&[69, 39], None),
    L(
        &[69, 40],
        Some("the SetZEOF property of Config.Miscellaneous"),
    ),
    L(
        &[69, 42],
        Some("the NodeNameInPid property of Config.Miscellaneous"),
    ),
    L(
        &[69, 43],
        Some("the OldZU5 property of Config.Miscellaneous"),
    ),
    L(
        &[69, 44],
        Some("the UseNagleAlgorithm property of Config.Miscellaneous"),
    ),
    L(
        &[69, 45],
        Some("the TruncateOverflow property of Config.Miscellaneous"),
    ),
    L(
        &[69, 49],
        Some("the LogRollback property of Config.Miscellaneous"),
    ),
    L(
        &[69, 51],
        Some("the SwitchOSdir property of Config.Miscellaneous"),
    ),
    L(&[69, 55], None),
    L(
        &[69, 60],
        Some("the AsyncDisconnectErr property of Config.Miscellaneous"),
    ),
    L(
        &[69, 63],
        Some("the ScientificNotation property of Config.Miscellaneous"),
    ),
    L(
        &[69, 66],
        Some("the TelnetNUL property of Config.Miscellaneous"),
    ),
    L(&[69, 67], None),
    L(
        &[69, 68],
        Some("the DBEncJournal property of Security.System"),
    ),
    L(
        &[69, 69],
        Some("the EnableLongStrings property of Config.Miscellaneous"),
    ),
    L(
        &[69, 70],
        Some("the IEEEError property of Config.Miscellaneous"),
    ),
    L(&[69, 71], Some("the IPv6 property of Config.Startup")),
    L(
        &[69, 72],
        Some("the MVDefined property of Config.Miscellaneous"),
    ),
    L(&[71], Some("$SYSTEM.Process.FixedDate()")),
    L(
        &[78, 21],
        Some("##class(%SYS.Journal.System).GetImageJournalInfo()"),
    ),
    L(&[78, 22], Some("the %SYS.Journal.System class methods")),
    L(&[78, 23], Some("##class(%SYS.Journal.File).PurgeOne()")),
    L(&[78, 28], None),
    L(&[78, 29], Some("##class(%SYS.Journal.System).Sync()")),
    L(
        &[78, 40],
        Some("##class(%SYS.Journal.System).WhereCommitted()"),
    ),
    L(&[82, 12], Some("##class(%Device).ReDirectIO()")),
    L(&[86], Some("##class(%SYS.System).GetCPFFileName()")),
    L(&[90, 4], None),
    L(&[90, 10], Some("##class(%SYS.Namespace).Exists()")),
    L(&[94], Some("$SYSTEM.Process.Broadcast()")),
    L(&[96, 3], Some("$SYSTEM.Process.ThrowError()")),
    L(&[96, 4], Some("$SYSTEM.Process.IODollarTest()")),
    L(&[96, 5], Some("$DEVICE")),
    L(&[96, 9], Some("$SYSTEM.Process.CallingRoutine()")),
    L(&[96, 10], Some("$SYSTEM.Process.CallingDatabase()")),
    L(&[96, 14], Some("##class(%Device).GetType()")),
    L(&[100], None),
    L(&[110], Some("##class(%SYS.System).GetNodeName()")),
    L(&[113], None),
    L(&[114], Some("$SYSTEM.INetInfo.EthernetAddress()")),
    L(
        &[115, 11],
        Some("$SYSTEM.SQL.GetIdentityInsert() or SetIdentityInsert()"),
    ),
    L(&[128, 1], Some("$SYSTEM.Process.StepInfo()")),
    L(&[130], None),
    L(&[132], Some("##class(%Device).ChangePrincipal()")),
    L(&[133], None),
    L(&[140, 1], Some("the %File class methods")),
    L(&[140, 7], Some("##class(%File).Attributes()")),
    L(&[147], Some("##class(%File).NormalizeFilenameWithSpaces()")),
    L(&[158, 0], Some("##class(%Device).InstalledPrinters()")),
    L(&[158, 1], Some("##class(%Device).InstalledPrinters()")),
    L(&[168], Some("$SYSTEM.Process.CurrentDirectory()")),
    L(&[186], Some("$SYSTEM.Process.TerminalPrompt()")),
    L(&[188], Some("$NOW")),
    L(&[189], Some("$SYSTEM.INetInfo.Connected()")),
    L(&[193], Some("$ZDATETIMEH")),
];
/// The entry for a call whose leading literal arguments are `args`, out of `count` arguments in
/// all: the longest identifying prefix that agrees on whether more arguments follow, a
/// definite answer about that beating an indifferent one.
pub(crate) fn lookup(args: &[u32], count: usize) -> Option<&'static Legacy> {
    LEGACY
        .iter()
        .filter(|l| args.starts_with(l.args))
        .filter(|l| l.more.is_none_or(|more| more == (count > l.args.len())))
        .max_by_key(|l| (l.args.len(), l.more.is_some()))
}
