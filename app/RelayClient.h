#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

@interface RelayConfigModel : NSObject <NSCopying>
@property (nonatomic, copy) NSString *url;
@property (nonatomic, copy, nullable) NSString *stateCode;
@property (nonatomic, copy, nullable) NSString *stateSecret;
@property (nonatomic, copy, nullable) NSString *serialNumber;
@property (nonatomic, copy, nullable) NSString *uniqueDeviceID;
@property (nonatomic, assign) BOOL useMacIdentity;

+ (instancetype)fromDictionary:(NSDictionary *)dict;
- (NSDictionary *)toDictionary;
@end

@interface RelayStatusModel : NSObject
@property (nonatomic, copy) NSString *status; // "connected", "connecting", "failed"
@property (nonatomic, copy, nullable) NSString *errorMessage;
@property (nonatomic, strong, nullable) NSNumber *retryInSecs;
@property (nonatomic, copy, nullable) NSString *liveCode;
@property (nonatomic, strong, nullable) NSNumber *connectedSince;
@property (nonatomic, assign) NSInteger pid;
@property (nonatomic, assign) BOOL pendingRestart;
@property (nonatomic, strong) RelayConfigModel *config;

+ (instancetype)fromDictionary:(NSDictionary *)dict;
@end

@interface RelayClient : NSObject
@property (nonatomic, copy, readonly) NSString *baseURL;

+ (instancetype)sharedClient;
- (instancetype)initWithBaseURL:(NSString *)baseURL;

- (void)fetchStatusWithCompletion:(void (^)(RelayStatusModel * _Nullable status, NSError * _Nullable error))completion;
- (void)saveConfig:(RelayConfigModel *)config
        completion:(void (^)(RelayConfigModel * _Nullable savedConfig, NSError * _Nullable error))completion;
- (void)restartDaemonWithCompletion:(void (^)(BOOL ok, NSError * _Nullable error))completion;
@end

NS_ASSUME_NONNULL_END
