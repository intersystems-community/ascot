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
const fn L(args: &'static [u32], more: Option<bool>, replacement: Option<&'static str>) -> Legacy {
    Legacy {
        args,
        more,
        replacement,
    }
}

pub(crate) const LEGACY: &[Legacy] = &[
    L(&[4], None, Some("$SYSTEM.Process.Terminate()")),
    L(&[5], None, Some("$NAMESPACE")),
    L(
        &[9],
        None,
        Some("##class(%Device).Broadcast() or ##class(%SYS.System).WriteToConsoleLog()"),
    ),
    L(&[12], None, Some("##class(%File).NormalizeDirectory()")),
    L(&[15], None, Some("##class(%File).NormalizeFilename()")),
    L(&[18], None, Some("$SYSTEM.Process.Undefined()")),
    L(&[20], None, Some("$SYSTEM.Process.UserRoutinePath()")),
    L(&[21], None, Some("$SYSTEM.Process.PrivateGlobalLocation()")),
    L(
        &[21, 0],
        None,
        Some("$SYSTEM.Process.PrivateGlobalLocation()"),
    ),
    L(
        &[21, 1],
        None,
        Some("$SYSTEM.Process.KillAllPrivateGlobals()"),
    ),
    L(
        &[21, 2],
        None,
        Some("$SYSTEM.Process.KillAllPrivateGlobals()"),
    ),
    L(&[22, 0], None, Some("##class(%Device).SetFFBS()")),
    L(&[28], None, Some("$SYSTEM.Util.Collation()")),
    L(&[39], None, Some("$SYSTEM.Process.SysRoutinePath()")),
    L(&[49], None, Some("the SYS.Database class properties")),
    L(&[53], Some(false), Some("$SYSTEM.INetInfo.TCPName()")),
    L(&[53], Some(true), Some("$SYSTEM.INetInfo.TCPStats()")),
    L(&[55], None, Some("$SYSTEM.Process.LanguageMode()")),
    L(&[56, 2], None, Some("$SYSTEM.Process.ErrorLine()")),
    L(&[56, 6], None, Some("$SYSTEM.Process.OSError()")),
    L(&[62], None, Some("##class(%Routine).CheckSyntax()")),
    L(&[67, 0], None, Some("$SYSTEM.Process.IsGhost()")),
    L(&[67, 1], None, None),
    L(&[67, 4], None, Some("$SYSTEM.Process.State()")),
    L(&[67, 5], None, Some("$SYSTEM.Process.Routine()")),
    L(&[67, 6], None, Some("$SYSTEM.Process.NameSpace()")),
    L(&[67, 7], None, Some("$SYSTEM.Process.CurrentDevice()")),
    L(&[67, 8], None, Some("$SYSTEM.Process.LinesExecuted()")),
    L(&[67, 9], None, Some("$SYSTEM.Process.GlobalReferences()")),
    L(&[67, 10], None, Some("$SYSTEM.Process.JobType()")),
    L(&[67, 11], None, Some("$SYSTEM.Process.UserName()")),
    L(&[67, 12], None, Some("$SYSTEM.Process.ClientNodeName()")),
    L(
        &[67, 13],
        None,
        Some("$SYSTEM.Process.ClientExecutableName()"),
    ),
    L(&[67, 14], None, Some("$SYSTEM.Process.CSPSessionID()")),
    L(&[67, 15], None, Some("$SYSTEM.Process.ClientIPAddress()")),
    L(&[68, 1], None, Some("$SYSTEM.Process.NullSubscripts()")),
    L(&[68, 2], None, Some("$SYSTEM.Process.OpenMode()")),
    L(&[68, 3], None, Some("$SYSTEM.Process.FileMode()")),
    L(&[68, 5], None, Some("$SYSTEM.Process.BreakMode()")),
    L(&[68, 6], None, None),
    L(&[68, 7], None, Some("$SYSTEM.Process.RefInKind()")),
    L(&[68, 11], None, Some("$SYSTEM.Process.LineRecall()")),
    L(&[68, 15], None, Some("$SYSTEM.Process.DisconnectErr()")),
    L(&[68, 21], None, Some("$SYSTEM.Process.SynchCommit()")),
    L(&[68, 22], None, Some("$SYSTEM.Process.DX()")),
    L(&[68, 25], None, Some("$SYSTEM.Process.BatchFlag()")),
    L(&[68, 26], None, Some("$SYSTEM.Process.TerminalPrompt()")),
    L(&[68, 27], None, None),
    L(
        &[68, 28],
        None,
        Some("$SYSTEM.Process.GlobalKillDisabled()"),
    ),
    L(&[68, 30], None, Some("$SYSTEM.Process.PopError()")),
    L(&[68, 32], None, Some("$SYSTEM.Process.ZDateNull()")),
    L(&[68, 34], None, Some("$SYSTEM.Process.AsynchError()")),
    L(&[68, 39], None, None),
    L(&[68, 40], None, Some("$SYSTEM.Process.SetZEOF()")),
    L(&[68, 42], None, Some("$SYSTEM.Process.NodeNameInPid()")),
    L(&[68, 43], None, Some("$SYSTEM.Process.OldZU5()")),
    L(&[68, 45], None, Some("$SYSTEM.Process.TruncateOverflow()")),
    L(&[68, 51], None, Some("$SYSTEM.Process.SwitchOSdir()")),
    L(&[68, 55], None, None),
    L(
        &[68, 60],
        None,
        Some("$SYSTEM.Process.AsyncDisconnectErr()"),
    ),
    L(
        &[68, 63],
        None,
        Some("$SYSTEM.Process.ScientificNotation()"),
    ),
    L(&[68, 66], None, Some("$SYSTEM.Process.TelnetNUL()")),
    L(&[68, 67], None, Some("$SYSTEM.Process.ExceptionLog()")),
    L(&[68, 70], None, Some("$SYSTEM.Process.IEEEError()")),
    L(&[68, 71], None, Some("$SYSTEM.Process.IPv6Format()")),
    L(&[68, 72], None, Some("$SYSTEM.Process.MVUndefined()")),
    L(
        &[69, 0],
        None,
        Some("the Undefined property of Config.Miscellaneous"),
    ),
    L(
        &[69, 1],
        None,
        Some("the NullSubscripts property of Config.Miscellaneous"),
    ),
    L(
        &[69, 2],
        None,
        Some("the OpenMode property of Config.Miscellaneous"),
    ),
    L(
        &[69, 3],
        None,
        Some("the FileMode property of Config.Miscellaneous"),
    ),
    L(
        &[69, 5],
        None,
        Some("the BreakMode property of Config.Miscellaneous"),
    ),
    L(&[69, 6], None, None),
    L(
        &[69, 7],
        None,
        Some("the RefInKind property of Config.Miscellaneous"),
    ),
    L(
        &[69, 8],
        None,
        Some("the ZaMode property of Config.Miscellaneous"),
    ),
    L(
        &[69, 10],
        None,
        Some("the FreezeOnError property of Config.Journal"),
    ),
    L(
        &[69, 11],
        None,
        Some("the LineRecall property of Config.Miscellaneous"),
    ),
    L(&[69, 13], None, None),
    L(&[69, 14], None, None),
    L(
        &[69, 15],
        None,
        Some("the DisconnectErr property of Config.Miscellaneous"),
    ),
    L(&[69, 19], None, None),
    L(&[69, 20], None, None),
    L(
        &[69, 21],
        None,
        Some("the SynchCommit property of Config.Miscellaneous"),
    ),
    L(
        &[69, 22],
        None,
        Some("the DX property of Config.Miscellaneous"),
    ),
    L(
        &[69, 26],
        None,
        Some("the TerminalPrompt property of Config.Startup"),
    ),
    L(&[69, 27], None, None),
    L(&[69, 28], None, None),
    L(
        &[69, 30],
        None,
        Some("the PopError property of Config.Miscellaneous"),
    ),
    L(&[69, 31], None, None),
    L(
        &[69, 32],
        None,
        Some("the ZDateNull property of Config.Miscellaneous"),
    ),
    L(
        &[69, 34],
        None,
        Some("the AsynchError property of Config.Miscellaneous"),
    ),
    L(&[69, 35], None, None),
    L(
        &[69, 37],
        None,
        Some("the PhysicalCursor property of Config.NLS.Locales"),
    ),
    L(&[69, 39], None, None),
    L(
        &[69, 40],
        None,
        Some("the SetZEOF property of Config.Miscellaneous"),
    ),
    L(
        &[69, 42],
        None,
        Some("the NodeNameInPid property of Config.Miscellaneous"),
    ),
    L(
        &[69, 43],
        None,
        Some("the OldZU5 property of Config.Miscellaneous"),
    ),
    L(
        &[69, 44],
        None,
        Some("the UseNagleAlgorithm property of Config.Miscellaneous"),
    ),
    L(
        &[69, 45],
        None,
        Some("the TruncateOverflow property of Config.Miscellaneous"),
    ),
    L(
        &[69, 49],
        None,
        Some("the LogRollback property of Config.Miscellaneous"),
    ),
    L(
        &[69, 51],
        None,
        Some("the SwitchOSdir property of Config.Miscellaneous"),
    ),
    L(&[69, 55], None, None),
    L(
        &[69, 60],
        None,
        Some("the AsyncDisconnectErr property of Config.Miscellaneous"),
    ),
    L(
        &[69, 63],
        None,
        Some("the ScientificNotation property of Config.Miscellaneous"),
    ),
    L(
        &[69, 66],
        None,
        Some("the TelnetNUL property of Config.Miscellaneous"),
    ),
    L(&[69, 67], None, None),
    L(
        &[69, 68],
        None,
        Some("the DBEncJournal property of Security.System"),
    ),
    L(
        &[69, 69],
        None,
        Some("the EnableLongStrings property of Config.Miscellaneous"),
    ),
    L(
        &[69, 70],
        None,
        Some("the IEEEError property of Config.Miscellaneous"),
    ),
    L(&[69, 71], None, Some("the IPv6 property of Config.Startup")),
    L(
        &[69, 72],
        None,
        Some("the MVDefined property of Config.Miscellaneous"),
    ),
    L(&[71], None, Some("$SYSTEM.Process.FixedDate()")),
    L(
        &[78, 21],
        None,
        Some("##class(%SYS.Journal.System).GetImageJournalInfo()"),
    ),
    L(
        &[78, 22],
        None,
        Some("the %SYS.Journal.System class methods"),
    ),
    L(
        &[78, 23],
        None,
        Some("##class(%SYS.Journal.File).PurgeOne()"),
    ),
    L(&[78, 28], None, None),
    L(&[78, 29], None, Some("##class(%SYS.Journal.System).Sync()")),
    L(
        &[78, 40],
        None,
        Some("##class(%SYS.Journal.System).WhereCommitted()"),
    ),
    L(&[82, 12], None, Some("##class(%Device).ReDirectIO()")),
    L(&[86], None, Some("##class(%SYS.System).GetCPFFileName()")),
    L(&[90, 4], None, None),
    L(&[90, 10], None, Some("##class(%SYS.Namespace).Exists()")),
    L(&[94], None, Some("$SYSTEM.Process.Broadcast()")),
    L(&[96, 3], None, Some("$SYSTEM.Process.ThrowError()")),
    L(&[96, 4], None, Some("$SYSTEM.Process.IODollarTest()")),
    L(&[96, 5], None, Some("$DEVICE")),
    L(&[96, 9], None, Some("$SYSTEM.Process.CallingRoutine()")),
    L(&[96, 10], None, Some("$SYSTEM.Process.CallingDatabase()")),
    L(&[96, 14], None, Some("##class(%Device).GetType()")),
    L(&[100], None, None),
    L(&[110], None, Some("##class(%SYS.System).GetNodeName()")),
    L(&[113], None, None),
    L(&[114], None, Some("$SYSTEM.INetInfo.EthernetAddress()")),
    L(
        &[115, 11],
        None,
        Some("$SYSTEM.SQL.GetIdentityInsert() or SetIdentityInsert()"),
    ),
    L(&[128, 1], None, Some("$SYSTEM.Process.StepInfo()")),
    L(&[130], None, None),
    L(&[132], None, Some("##class(%Device).ChangePrincipal()")),
    L(&[133], None, None),
    L(&[140, 1], None, Some("the %File class methods")),
    L(&[140, 7], None, Some("##class(%File).Attributes()")),
    L(
        &[147],
        None,
        Some("##class(%File).NormalizeFilenameWithSpaces()"),
    ),
    L(
        &[158, 0],
        None,
        Some("##class(%Device).InstalledPrinters()"),
    ),
    L(
        &[158, 1],
        None,
        Some("##class(%Device).InstalledPrinters()"),
    ),
    L(&[168], None, Some("$SYSTEM.Process.CurrentDirectory()")),
    L(&[186], None, Some("$SYSTEM.Process.TerminalPrompt()")),
    L(&[188], None, Some("$NOW")),
    L(&[189], None, Some("$SYSTEM.INetInfo.Connected()")),
    L(&[193], None, Some("$ZDATETIMEH")),
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
