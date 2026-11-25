#import "RelayViewController.h"
#import "RelayClient.h"

static BOOL RSIsIOS13OrNewer(void) {
    static BOOL isIOS13 = NO;
    static dispatch_once_t onceToken;
    dispatch_once(&onceToken, ^{
        NSOperatingSystemVersion v13 = {13, 0, 0};
        isIOS13 = [[NSProcessInfo processInfo] isOperatingSystemAtLeastVersion:v13];
    });
    return isIOS13;
}

#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wunguarded-availability-new"

static UIColor *RSLabelColor(void) {
    if (RSIsIOS13OrNewer() && [UIColor respondsToSelector:@selector(labelColor)]) {
        return [UIColor labelColor];
    }
    return [UIColor blackColor];
}

static UIColor *RSSecondaryLabelColor(void) {
    if (RSIsIOS13OrNewer() && [UIColor respondsToSelector:@selector(secondaryLabelColor)]) {
        return [UIColor secondaryLabelColor];
    }
    return [UIColor darkGrayColor];
}

static UIColor *RSTertiaryLabelColor(void) {
    if (RSIsIOS13OrNewer() && [UIColor respondsToSelector:@selector(tertiaryLabelColor)]) {
        return [UIColor tertiaryLabelColor];
    }
    return [UIColor lightGrayColor];
}

static UIFont *RSMonospacedFont(CGFloat size, UIFontWeight weight) {
    if (RSIsIOS13OrNewer() && [UIFont respondsToSelector:@selector(monospacedSystemFontOfSize:weight:)]) {
        return [UIFont monospacedSystemFontOfSize:size weight:weight];
    }
    NSString *name = (weight >= UIFontWeightMedium) ? @"Menlo-Bold" : @"Menlo-Regular";
    return [UIFont fontWithName:name size:size] ?: [UIFont systemFontOfSize:size weight:weight];
}

#pragma clang diagnostic pop

typedef NS_ENUM(NSInteger, RelaySection) {
    RelaySectionStatus = 0,
    RelaySectionDaemonControl = 1,
    RelaySectionConfig = 2,
    RelaySectionActions = 3,
    RelaySectionCount = 4,
};

typedef NS_ENUM(NSInteger, StatusRow) {
    StatusRowConnection = 0,
    StatusRowRelayCode = 1,
    StatusRowRelayURL = 2,
    StatusRowPendingRestart = 3,
};

typedef NS_ENUM(NSInteger, ConfigRow) {
    ConfigRowUseMacIdentity = 0,
    ConfigRowSerialNumber = 1,
    ConfigRowUniqueDeviceID = 2,
    ConfigRowCount = 3,
};

typedef NS_ENUM(NSInteger, TextFieldTag) {
    TextFieldTagURL = 100,
    TextFieldTagSerialNumber = 101,
    TextFieldTagUniqueDeviceID = 102,
};

typedef NS_ENUM(NSInteger, ActionRow) {
    ActionRowSave = 0,
    ActionRowSaveAndRestart = 1,
    ActionRowReload = 2,
    ActionRowCount = 3,
};

@interface RelayViewController () <UITextFieldDelegate>
@property (nonatomic, strong, nullable) RelayStatusModel *latestStatus;
@property (nonatomic, strong) RelayConfigModel *draftConfig;
@property (nonatomic, copy, nullable) NSString *connectionError;
@property (nonatomic, assign) BOOL hasLoadedConfig;
@property (nonatomic, assign) BOOL hasUnsavedChanges;
@property (nonatomic, assign) BOOL isEditingTextField;
@property (nonatomic, assign) BOOL isRestarting;
@property (nonatomic, assign) BOOL isSaving;
@property (nonatomic, assign) BOOL justCopiedCode;
@property (nonatomic, assign) BOOL showingPendingRestartRow;
@property (nonatomic, assign) NSInteger pidBeforeRestart;
@property (nonatomic, strong, nullable) NSTimer *pollTimer;
@property (nonatomic, strong) UIBarButtonItem *saveBarButton;
@end

@implementation RelayViewController

- (instancetype)init {
    if (RSIsIOS13OrNewer()) {
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wunguarded-availability-new"
        return [super initWithStyle:UITableViewStyleInsetGrouped];
#pragma clang diagnostic pop
    }
    return [super initWithStyle:UITableViewStyleGrouped];
}

- (void)viewDidLoad {
    [super viewDidLoad];
    self.title = @"RelayServer";
    self.draftConfig = [[RelayConfigModel alloc] init];
    self.tableView.keyboardDismissMode = UIScrollViewKeyboardDismissModeInteractive;

    self.saveBarButton = [[UIBarButtonItem alloc] initWithTitle:@"Save"
                                                          style:UIBarButtonItemStyleDone
                                                         target:self
                                                         action:@selector(saveTapped)];
    self.saveBarButton.enabled = NO;
    self.navigationItem.rightBarButtonItem = self.saveBarButton;

    UIRefreshControl *refresh = [[UIRefreshControl alloc] init];
    [refresh addTarget:self action:@selector(pullToRefresh) forControlEvents:UIControlEventValueChanged];
    self.refreshControl = refresh;

    [[NSNotificationCenter defaultCenter] addObserver:self
                                             selector:@selector(startPolling)
                                                 name:UIApplicationDidBecomeActiveNotification
                                               object:nil];
    [[NSNotificationCenter defaultCenter] addObserver:self
                                             selector:@selector(stopPolling)
                                                 name:UIApplicationWillResignActiveNotification
                                               object:nil];

    [self fetchStatusForceReloadConfig:NO];
    [self startPolling];
}

- (void)dealloc {
    [self stopPolling];
    [[NSNotificationCenter defaultCenter] removeObserver:self];
}

#pragma mark - Polling

- (void)startPolling {
    [self stopPolling];
    __weak typeof(self) weakSelf = self;
    self.pollTimer = [NSTimer scheduledTimerWithTimeInterval:3.0
                                                     repeats:YES
                                                       block:^(NSTimer * _Nonnull timer) {
        [weakSelf fetchStatusForceReloadConfig:NO];
    }];
}

- (void)stopPolling {
    [self.pollTimer invalidate];
    self.pollTimer = nil;
}

- (void)pullToRefresh {
    [self fetchStatusForceReloadConfig:NO];
}

- (void)fetchStatusForceReloadConfig:(BOOL)forceReloadConfig {
    __weak typeof(self) weakSelf = self;
    [[RelayClient sharedClient] fetchStatusWithCompletion:^(RelayStatusModel * _Nullable status, NSError * _Nullable error) {
        __strong typeof(weakSelf) self = weakSelf;
        if (!self) return;

        [self.refreshControl endRefreshing];

        if (error || !status) {
            self.latestStatus = nil;
            self.connectionError = error.localizedDescription ?: @"Daemon unreachable";
            [self updateStatusSectionAndControls];
            return;
        }

        self.connectionError = nil;
        if (self.isRestarting && (self.pidBeforeRestart == 0 || status.pid != self.pidBeforeRestart)) {
            self.isRestarting = NO;
            self.pidBeforeRestart = 0;
        }

        self.latestStatus = status;

        BOOL configChanged = (status.config != nil) && ![self.draftConfig isEqualToConfig:status.config];
        BOOL shouldSyncConfig = forceReloadConfig
            || !self.hasLoadedConfig
            || (!self.hasUnsavedChanges && !self.isEditingTextField && configChanged);
        if (shouldSyncConfig && status.config) {
            self.draftConfig = [status.config copy];
            self.hasLoadedConfig = YES;
            if (forceReloadConfig) {
                self.hasUnsavedChanges = NO;
            }
            self.showingPendingRestartRow = status.pendingRestart;
            self.saveBarButton.enabled = self.hasUnsavedChanges && !self.isSaving;
            [self.tableView reloadData];
        } else {
            // Only refresh non-editable status rows + daemon control section so active text editing is never interrupted.
            [self updateStatusSectionAndControls];
        }
    }];
}

- (void)updateStatusSectionAndControls {
    self.saveBarButton.enabled = self.hasUnsavedChanges && !self.isSaving;
    if (self.isEditingTextField) {
        // Avoid mutating the table view while a text field is focused; on iOS 16
        // beginUpdates/endUpdates dismisses UIEditMenuInteraction (Copy/Paste menu).
        return;
    }
    BOOL shouldShowPending = (self.latestStatus != nil && self.latestStatus.pendingRestart);
    BOOL wasShowingPending = self.showingPendingRestartRow;
    self.showingPendingRestartRow = shouldShowPending;

    [UIView performWithoutAnimation:^{
        [self.tableView beginUpdates];
        NSArray<NSIndexPath *> *statusRows = @[
            [NSIndexPath indexPathForRow:StatusRowConnection inSection:RelaySectionStatus],
            [NSIndexPath indexPathForRow:StatusRowRelayCode inSection:RelaySectionStatus],
        ];
        [self.tableView reloadRowsAtIndexPaths:statusRows withRowAnimation:UITableViewRowAnimationNone];

        NSIndexPath *pendingPath = [NSIndexPath indexPathForRow:StatusRowPendingRestart inSection:RelaySectionStatus];
        if (shouldShowPending && !wasShowingPending) {
            [self.tableView insertRowsAtIndexPaths:@[pendingPath] withRowAnimation:UITableViewRowAnimationNone];
        } else if (!shouldShowPending && wasShowingPending) {
            [self.tableView deleteRowsAtIndexPaths:@[pendingPath] withRowAnimation:UITableViewRowAnimationNone];
        } else if (shouldShowPending && wasShowingPending) {
            [self.tableView reloadRowsAtIndexPaths:@[pendingPath] withRowAnimation:UITableViewRowAnimationNone];
        }

        [self.tableView reloadSections:[NSIndexSet indexSetWithIndex:RelaySectionDaemonControl]
                      withRowAnimation:UITableViewRowAnimationNone];
        [self.tableView endUpdates];
    }];
}

#pragma mark - Actions

- (void)saveTapped {
    [self.view endEditing:YES];
    [self performSaveAndOptionallyRestart:NO];
}

- (void)performSaveAndOptionallyRestart:(BOOL)shouldRestart {
    if (self.isSaving) return;
    self.isSaving = YES;
    self.saveBarButton.enabled = NO;

    __weak typeof(self) weakSelf = self;
    [[RelayClient sharedClient] saveConfig:self.draftConfig
                                completion:^(RelayConfigModel * _Nullable savedConfig, NSError * _Nullable error) {
        __strong typeof(weakSelf) self = weakSelf;
        if (!self) return;

        self.isSaving = NO;
        if (error) {
            self.saveBarButton.enabled = self.hasUnsavedChanges;
            [self showAlertWithTitle:@"Save Failed" message:error.localizedDescription];
            return;
        }

        if (savedConfig) {
            self.draftConfig = [savedConfig copy];
        }
        self.hasUnsavedChanges = NO;
        self.saveBarButton.enabled = NO;

        if (shouldRestart) {
            [self triggerDaemonRestart];
        } else {
            [self fetchStatusForceReloadConfig:YES];
            [self showAlertWithTitle:@"Configuration Saved"
                             message:@"Updated /var/mobile/config.json. Restart the daemon to apply changes."];
        }
    }];
}

- (void)confirmAndRestartDaemon {
    [self.view endEditing:YES];
    UIAlertController *alert = [UIAlertController alertControllerWithTitle:@"Restart Daemon?"
                                                                   message:@"The background relayserver daemon will exit and be immediately restarted by launchd."
                                                            preferredStyle:UIAlertControllerStyleAlert];
    [alert addAction:[UIAlertAction actionWithTitle:@"Cancel" style:UIAlertActionStyleCancel handler:nil]];
    __weak typeof(self) weakSelf = self;
    [alert addAction:[UIAlertAction actionWithTitle:@"Restart"
                                              style:UIAlertActionStyleDestructive
                                            handler:^(UIAlertAction * _Nonnull action) {
        [weakSelf triggerDaemonRestart];
    }]];
    [self presentViewController:alert animated:YES completion:nil];
}

- (void)triggerDaemonRestart {
    self.isRestarting = YES;
    self.pidBeforeRestart = self.latestStatus ? self.latestStatus.pid : 0;
    [self updateStatusSectionAndControls];

    __weak typeof(self) weakSelf = self;
    [[RelayClient sharedClient] restartDaemonWithCompletion:^(BOOL ok, NSError * _Nullable error) {
        __strong typeof(weakSelf) self = weakSelf;
        if (!self) return;

        if (!ok && error) {
            self.isRestarting = NO;
            [self updateStatusSectionAndControls];
            [self showAlertWithTitle:@"Restart Failed" message:error.localizedDescription];
            return;
        }

        // Poll shortly after restart so the UI picks up the new daemon process quickly.
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(1.0 * NSEC_PER_SEC)), dispatch_get_main_queue(), ^{
            [weakSelf fetchStatusForceReloadConfig:YES];
        });
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(2.5 * NSEC_PER_SEC)), dispatch_get_main_queue(), ^{
            [weakSelf fetchStatusForceReloadConfig:NO];
        });
    }];
}

- (void)showAlertWithTitle:(NSString *)title message:(NSString *)message {
    UIAlertController *alert = [UIAlertController alertControllerWithTitle:title
                                                                   message:message
                                                            preferredStyle:UIAlertControllerStyleAlert];
    [alert addAction:[UIAlertAction actionWithTitle:@"OK" style:UIAlertActionStyleDefault handler:nil]];
    [self presentViewController:alert animated:YES completion:nil];
}

- (NSString *)formattedUptimeFromUnix:(NSNumber *)connectedSince {
    if (!connectedSince) return nil;
    NSTimeInterval now = [[NSDate date] timeIntervalSince1970];
    NSInteger elapsed = MAX(0, (NSInteger)(now - connectedSince.doubleValue));
    NSInteger hours = elapsed / 3600;
    NSInteger mins = (elapsed % 3600) / 60;
    NSInteger secs = elapsed % 60;
    if (hours > 0) {
        return [NSString stringWithFormat:@"%ldh %ldm", (long)hours, (long)mins];
    } else if (mins > 0) {
        return [NSString stringWithFormat:@"%ldm %lds", (long)mins, (long)secs];
    }
    return [NSString stringWithFormat:@"%lds", (long)secs];
}

#pragma mark - UITableViewDataSource

- (NSInteger)numberOfSectionsInTableView:(UITableView *)tableView {
    return RelaySectionCount;
}

- (NSInteger)tableView:(UITableView *)tableView numberOfRowsInSection:(NSInteger)section {
    switch ((RelaySection)section) {
        case RelaySectionStatus:
            return self.showingPendingRestartRow ? 4 : 3;
        case RelaySectionDaemonControl:
            return 1;
        case RelaySectionConfig:
            return ConfigRowCount;
        case RelaySectionActions:
            return ActionRowCount;
        default:
            return 0;
    }
}

- (NSString *)tableView:(UITableView *)tableView titleForHeaderInSection:(NSInteger)section {
    switch ((RelaySection)section) {
        case RelaySectionStatus:
            return @"WebSocket & Daemon Status";
        case RelaySectionDaemonControl:
            return @"Daemon Control";
        case RelaySectionConfig:
            return @"Configuration";
        case RelaySectionActions:
            return @"Apply Changes";
        default:
            return nil;
    }
}

- (NSString *)tableView:(UITableView *)tableView titleForFooterInSection:(NSInteger)section {
    switch ((RelaySection)section) {
        case RelaySectionStatus:
            return @"The relayserver daemon runs independently in the background and stays active when this app is closed.";
        case RelaySectionConfig:
            return @"Leave optional fields blank to auto-generate on next daemon start. Serial number must be 12 characters if set.";
        default:
            return nil;
    }
}

- (UITableViewCell *)tableView:(UITableView *)tableView cellForRowAtIndexPath:(NSIndexPath *)indexPath {
    switch ((RelaySection)indexPath.section) {
        case RelaySectionStatus:
            return [self statusCellForRow:indexPath.row];
        case RelaySectionDaemonControl:
            return [self daemonControlCell];
        case RelaySectionConfig:
            return [self configCellForRow:(ConfigRow)indexPath.row];
        case RelaySectionActions:
            return [self actionCellForRow:(ActionRow)indexPath.row];
        default:
            return [[UITableViewCell alloc] init];
    }
}

- (UITableViewCell *)statusCellForRow:(NSInteger)row {
    if (row == 0) {
        UITableViewCell *cell = [[UITableViewCell alloc] initWithStyle:UITableViewCellStyleSubtitle reuseIdentifier:nil];
        cell.selectionStyle = UITableViewCellSelectionStyleNone;

        UIColor *dotColor = [UIColor systemGrayColor];
        NSString *title = @"Checking…";
        NSString *detail = @"Connecting to local daemon…";

        if (self.isRestarting) {
            dotColor = [UIColor systemOrangeColor];
            title = @"Restarting Daemon…";
            detail = @"Waiting for launchd to respawn relayserver";
        } else if (self.connectionError != nil || self.latestStatus == nil) {
            dotColor = [UIColor systemRedColor];
            title = @"Daemon Offline";
            detail = self.connectionError ?: @"Cannot reach daemon on 127.0.0.1:17420";
        } else if ([self.latestStatus.status isEqualToString:@"connected"]) {
            dotColor = [UIColor systemGreenColor];
            title = @"Connected";
            NSString *uptime = [self formattedUptimeFromUnix:self.latestStatus.connectedSince];
            if (uptime) {
                detail = [NSString stringWithFormat:@"WebSocket active · Up %@ · PID %ld", uptime, (long)self.latestStatus.pid];
            } else {
                detail = [NSString stringWithFormat:@"WebSocket active · PID %ld", (long)self.latestStatus.pid];
            }
        } else if ([self.latestStatus.status isEqualToString:@"connecting"]) {
            dotColor = [UIColor systemOrangeColor];
            title = @"Connecting…";
            detail = [NSString stringWithFormat:@"Establishing WebSocket connection · PID %ld", (long)self.latestStatus.pid];
        } else if ([self.latestStatus.status isEqualToString:@"failed"]) {
            dotColor = [UIColor systemRedColor];
            title = @"Connection Failed";
            NSString *err = self.latestStatus.errorMessage ?: @"Unknown error";
            if (self.latestStatus.retryInSecs) {
                detail = [NSString stringWithFormat:@"%@ (retrying in %@s)", err, self.latestStatus.retryInSecs];
            } else {
                detail = err;
            }
        } else {
            dotColor = [UIColor systemGrayColor];
            title = self.latestStatus.status ?: @"Unknown";
            detail = [NSString stringWithFormat:@"PID %ld", (long)self.latestStatus.pid];
        }

        cell.textLabel.text = title;
        cell.textLabel.font = [UIFont systemFontOfSize:17 weight:UIFontWeightSemibold];
        cell.detailTextLabel.text = detail;
        cell.detailTextLabel.textColor = RSSecondaryLabelColor();
        cell.detailTextLabel.numberOfLines = 0;

        UIView *dot = [[UIView alloc] initWithFrame:CGRectMake(0, 0, 12, 12)];
        dot.backgroundColor = dotColor;
        dot.layer.cornerRadius = 6;
        cell.accessoryView = dot;
        return cell;
    }

    if (row == StatusRowRelayCode) {
        UITableViewCell *cell = [[UITableViewCell alloc] initWithStyle:UITableViewCellStyleValue1 reuseIdentifier:nil];
        cell.textLabel.text = @"Relay Code";
        NSString *code = self.latestStatus.liveCode ?: self.latestStatus.config.stateCode;
        if (code.length > 0) {
            cell.detailTextLabel.text = self.justCopiedCode ? @"Copied!" : code;
            cell.detailTextLabel.font = RSMonospacedFont(15, UIFontWeightMedium);
            cell.detailTextLabel.textColor = self.justCopiedCode ? [UIColor systemGreenColor] : RSLabelColor();
            cell.selectionStyle = UITableViewCellSelectionStyleDefault;
        } else {
            cell.detailTextLabel.text = @"Not registered";
            cell.detailTextLabel.textColor = RSSecondaryLabelColor();
            cell.selectionStyle = UITableViewCellSelectionStyleNone;
        }
        return cell;
    }

    if (row == StatusRowRelayURL) {
        return [self textFieldCellWithLabel:@"Relay URL"
                                       text:self.draftConfig.url
                                placeholder:@"wss://registration-relay.beeper.com/api/v1/provider"
                                        tag:TextFieldTagURL
                               keyboardType:UIKeyboardTypeURL
                               autocapType:UITextAutocapitalizationTypeNone
                                       font:[UIFont systemFontOfSize:15]];
    }

    // StatusRowPendingRestart: Pending restart banner
    UITableViewCell *cell = [[UITableViewCell alloc] initWithStyle:UITableViewCellStyleSubtitle reuseIdentifier:nil];
    cell.selectionStyle = UITableViewCellSelectionStyleNone;
    cell.textLabel.text = @"Restart Required";
    cell.textLabel.textColor = [UIColor systemOrangeColor];
    cell.textLabel.font = [UIFont systemFontOfSize:15 weight:UIFontWeightSemibold];
    cell.detailTextLabel.text = @"Saved configuration changes will take effect after restarting the daemon.";
    cell.detailTextLabel.textColor = RSSecondaryLabelColor();
    cell.detailTextLabel.numberOfLines = 0;
    return cell;
}

- (UITableViewCell *)daemonControlCell {
    UITableViewCell *cell = [[UITableViewCell alloc] initWithStyle:UITableViewCellStyleDefault reuseIdentifier:nil];
    cell.textLabel.text = self.isRestarting ? @"Restarting Daemon…" : @"Restart Daemon";
    cell.textLabel.textAlignment = NSTextAlignmentCenter;
    cell.textLabel.font = [UIFont systemFontOfSize:17 weight:UIFontWeightMedium];
    BOOL canRestart = !self.isRestarting && (self.latestStatus != nil);
    cell.textLabel.textColor = canRestart ? [UIColor systemRedColor] : RSTertiaryLabelColor();
    cell.selectionStyle = canRestart ? UITableViewCellSelectionStyleDefault : UITableViewCellSelectionStyleNone;

    if (self.isRestarting) {
        UIActivityIndicatorViewStyle style;
        if (RSIsIOS13OrNewer()) {
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wunguarded-availability-new"
            style = UIActivityIndicatorViewStyleMedium;
#pragma clang diagnostic pop
        } else {
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
            style = UIActivityIndicatorViewStyleGray;
#pragma clang diagnostic pop
        }
        UIActivityIndicatorView *spinner = [[UIActivityIndicatorView alloc] initWithActivityIndicatorStyle:style];
        [spinner startAnimating];
        cell.accessoryView = spinner;
    }
    return cell;
}

- (UITableViewCell *)textFieldCellWithLabel:(NSString *)title
                                       text:(NSString * _Nullable)text
                                placeholder:(NSString *)placeholder
                                        tag:(NSInteger)tag
                               keyboardType:(UIKeyboardType)keyboardType
                                autocapType:(UITextAutocapitalizationType)autocapType
                                       font:(UIFont *)font {
    UITableViewCell *cell = [[UITableViewCell alloc] initWithStyle:UITableViewCellStyleDefault reuseIdentifier:nil];
    cell.selectionStyle = UITableViewCellSelectionStyleNone;

    UILabel *label = [[UILabel alloc] init];
    label.text = title;
    label.font = [UIFont systemFontOfSize:13 weight:UIFontWeightMedium];
    label.textColor = RSSecondaryLabelColor();
    label.translatesAutoresizingMaskIntoConstraints = NO;

    UITextField *field = [[UITextField alloc] init];
    field.tag = tag;
    field.text = text;
    field.placeholder = placeholder;
    field.keyboardType = keyboardType;
    field.autocapitalizationType = autocapType;
    field.font = font;
    field.delegate = self;
    field.autocorrectionType = UITextAutocorrectionTypeNo;
    field.clearButtonMode = UITextFieldViewModeWhileEditing;
    field.returnKeyType = UIReturnKeyDone;
    field.translatesAutoresizingMaskIntoConstraints = NO;
    [field addTarget:self action:@selector(textFieldDidChange:) forControlEvents:UIControlEventEditingChanged];

    [cell.contentView addSubview:label];
    [cell.contentView addSubview:field];

    [NSLayoutConstraint activateConstraints:@[
        [label.topAnchor constraintEqualToAnchor:cell.contentView.topAnchor constant:8],
        [label.leadingAnchor constraintEqualToAnchor:cell.contentView.layoutMarginsGuide.leadingAnchor],
        [label.trailingAnchor constraintEqualToAnchor:cell.contentView.layoutMarginsGuide.trailingAnchor],

        [field.topAnchor constraintEqualToAnchor:label.bottomAnchor constant:4],
        [field.leadingAnchor constraintEqualToAnchor:cell.contentView.layoutMarginsGuide.leadingAnchor],
        [field.trailingAnchor constraintEqualToAnchor:cell.contentView.layoutMarginsGuide.trailingAnchor],
        [field.bottomAnchor constraintEqualToAnchor:cell.contentView.bottomAnchor constant:-10],
        [field.heightAnchor constraintGreaterThanOrEqualToConstant:26],
    ]];

    return cell;
}

- (UITableViewCell *)configCellForRow:(ConfigRow)row {
    if (row == ConfigRowUseMacIdentity) {
        UITableViewCell *cell = [[UITableViewCell alloc] initWithStyle:UITableViewCellStyleDefault reuseIdentifier:nil];
        cell.selectionStyle = UITableViewCellSelectionStyleNone;
        cell.textLabel.text = @"Use Mac Identity";
        UISwitch *toggle = [[UISwitch alloc] init];
        toggle.on = self.draftConfig.useMacIdentity;
        [toggle addTarget:self action:@selector(macIdentitySwitchChanged:) forControlEvents:UIControlEventValueChanged];
        cell.accessoryView = toggle;
        return cell;
    }

    switch (row) {
        case ConfigRowSerialNumber:
            return [self textFieldCellWithLabel:@"Serial Number"
                                           text:self.draftConfig.serialNumber
                                    placeholder:@"Auto-generated if blank (12 chars)"
                                            tag:TextFieldTagSerialNumber
                                   keyboardType:UIKeyboardTypeDefault
                                    autocapType:UITextAutocapitalizationTypeAllCharacters
                                           font:RSMonospacedFont(14, UIFontWeightRegular)];
        case ConfigRowUniqueDeviceID:
            return [self textFieldCellWithLabel:@"Unique Device ID"
                                           text:self.draftConfig.uniqueDeviceID
                                    placeholder:@"Auto-generated UUID if blank"
                                            tag:TextFieldTagUniqueDeviceID
                                   keyboardType:UIKeyboardTypeDefault
                                    autocapType:UITextAutocapitalizationTypeAllCharacters
                                           font:RSMonospacedFont(13, UIFontWeightRegular)];
        default:
            return [[UITableViewCell alloc] init];
    }
}

- (UITableViewCell *)actionCellForRow:(ActionRow)row {
    UITableViewCell *cell = [[UITableViewCell alloc] initWithStyle:UITableViewCellStyleDefault reuseIdentifier:nil];
    cell.textLabel.textAlignment = NSTextAlignmentCenter;
    cell.textLabel.font = [UIFont systemFontOfSize:17 weight:UIFontWeightMedium];

    BOOL daemonOnline = (self.latestStatus != nil) && !self.isSaving && !self.isRestarting;

    switch (row) {
        case ActionRowSave:
            cell.textLabel.text = self.isSaving ? @"Saving…" : @"Save Configuration";
            cell.textLabel.textColor = daemonOnline ? [UIColor systemBlueColor] : RSTertiaryLabelColor();
            cell.selectionStyle = daemonOnline ? UITableViewCellSelectionStyleDefault : UITableViewCellSelectionStyleNone;
            break;
        case ActionRowSaveAndRestart:
            cell.textLabel.text = @"Save & Restart Daemon";
            cell.textLabel.textColor = daemonOnline ? [UIColor systemBlueColor] : RSTertiaryLabelColor();
            cell.selectionStyle = daemonOnline ? UITableViewCellSelectionStyleDefault : UITableViewCellSelectionStyleNone;
            break;
        case ActionRowReload:
            cell.textLabel.text = @"Reload from Daemon";
            cell.textLabel.textColor = RSSecondaryLabelColor();
            cell.selectionStyle = UITableViewCellSelectionStyleDefault;
            break;
        default:
            break;
    }
    return cell;
}

#pragma mark - UITableViewDelegate

- (void)tableView:(UITableView *)tableView didSelectRowAtIndexPath:(NSIndexPath *)indexPath {
    [tableView deselectRowAtIndexPath:indexPath animated:YES];

    if (indexPath.section == RelaySectionStatus && indexPath.row == 1) {
        NSString *code = self.latestStatus.liveCode ?: self.latestStatus.config.stateCode;
        if (code.length > 0) {
            [UIPasteboard generalPasteboard].string = code;
            self.justCopiedCode = YES;
            [self updateStatusSectionAndControls];
            __weak typeof(self) weakSelf = self;
            dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(1.5 * NSEC_PER_SEC)), dispatch_get_main_queue(), ^{
                weakSelf.justCopiedCode = NO;
                [weakSelf updateStatusSectionAndControls];
            });
        }
        return;
    }

    if (indexPath.section == RelaySectionDaemonControl) {
        if (!self.isRestarting && self.latestStatus != nil) {
            [self confirmAndRestartDaemon];
        }
        return;
    }

    if (indexPath.section == RelaySectionActions) {
        switch ((ActionRow)indexPath.row) {
            case ActionRowSave:
                if (self.latestStatus != nil && !self.isSaving && !self.isRestarting) {
                    [self.view endEditing:YES];
                    [self performSaveAndOptionallyRestart:NO];
                }
                break;
            case ActionRowSaveAndRestart:
                if (self.latestStatus != nil && !self.isSaving && !self.isRestarting) {
                    [self.view endEditing:YES];
                    [self performSaveAndOptionallyRestart:YES];
                }
                break;
            case ActionRowReload:
                [self.view endEditing:YES];
                [self fetchStatusForceReloadConfig:YES];
                break;
            default:
                break;
        }
    }
}

#pragma mark - Input Handlers

- (void)macIdentitySwitchChanged:(UISwitch *)sender {
    self.draftConfig.useMacIdentity = sender.isOn;
    self.hasUnsavedChanges = YES;
    self.saveBarButton.enabled = !self.isSaving;
}

- (void)textFieldDidChange:(UITextField *)textField {
    NSString *val = textField.text ?: @"";
    switch ((TextFieldTag)textField.tag) {
        case TextFieldTagURL:
            self.draftConfig.url = val;
            break;
        case TextFieldTagSerialNumber:
            self.draftConfig.serialNumber = val;
            break;
        case TextFieldTagUniqueDeviceID:
            self.draftConfig.uniqueDeviceID = val;
            break;
        default:
            break;
    }
    self.hasUnsavedChanges = YES;
    self.saveBarButton.enabled = !self.isSaving;
}

- (void)textFieldDidBeginEditing:(UITextField *)textField {
    self.isEditingTextField = YES;
}

- (void)textFieldDidEndEditing:(UITextField *)textField {
    self.isEditingTextField = NO;
    [self updateStatusSectionAndControls];
}

- (BOOL)textFieldShouldReturn:(UITextField *)textField {
    [textField resignFirstResponder];
    return YES;
}

@end
