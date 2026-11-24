#import "RelayClient.h"

static NSString * const kRelayClientErrorDomain = @"dev.copper.relayserver.app";
static NSString * const kDefaultBaseURL = @"http://127.0.0.1:17420";

static NSString * _Nullable StringOrNil(id value) {
    if ([value isKindOfClass:[NSString class]]) {
        return (NSString *)value;
    }
    return nil;
}

static NSNumber * _Nullable NumberOrNil(id value) {
    if ([value isKindOfClass:[NSNumber class]]) {
        return (NSNumber *)value;
    }
    return nil;
}

@implementation RelayConfigModel

+ (instancetype)fromDictionary:(NSDictionary *)dict {
    RelayConfigModel *model = [[RelayConfigModel alloc] init];
    model.url = StringOrNil(dict[@"url"]) ?: @"";

    id stateObj = dict[@"state"];
    if ([stateObj isKindOfClass:[NSDictionary class]]) {
        NSDictionary *stateDict = (NSDictionary *)stateObj;
        model.stateCode = StringOrNil(stateDict[@"code"]);
        model.stateSecret = StringOrNil(stateDict[@"secret"]);
    }

    model.serialNumber = StringOrNil(dict[@"serial_number"]);
    model.uniqueDeviceID = StringOrNil(dict[@"unique_device_id"]);
    NSNumber *useMac = NumberOrNil(dict[@"use_mac_identity"]);
    model.useMacIdentity = useMac ? useMac.boolValue : NO;
    return model;
}

- (NSDictionary *)toDictionary {
    NSMutableDictionary *dict = [NSMutableDictionary dictionary];
    dict[@"url"] = self.url ?: @"";

    NSString *code = [self.stateCode stringByTrimmingCharactersInSet:[NSCharacterSet whitespaceAndNewlineCharacterSet]];
    NSString *secret = [self.stateSecret stringByTrimmingCharactersInSet:[NSCharacterSet whitespaceAndNewlineCharacterSet]];
    if (code.length > 0 || secret.length > 0) {
        dict[@"state"] = @{
            @"code": code ?: @"",
            @"secret": secret ?: @"",
        };
    } else {
        dict[@"state"] = [NSNull null];
    }

    NSString *serial = [self.serialNumber stringByTrimmingCharactersInSet:[NSCharacterSet whitespaceAndNewlineCharacterSet]];
    dict[@"serial_number"] = serial.length > 0 ? serial : [NSNull null];

    NSString *udid = [self.uniqueDeviceID stringByTrimmingCharactersInSet:[NSCharacterSet whitespaceAndNewlineCharacterSet]];
    dict[@"unique_device_id"] = udid.length > 0 ? udid : [NSNull null];

    dict[@"use_mac_identity"] = @(self.useMacIdentity);
    return dict;
}

- (id)copyWithZone:(NSZone *)zone {
    RelayConfigModel *copy = [[RelayConfigModel allocWithZone:zone] init];
    copy.url = [self.url copyWithZone:zone];
    copy.stateCode = [self.stateCode copyWithZone:zone];
    copy.stateSecret = [self.stateSecret copyWithZone:zone];
    copy.serialNumber = [self.serialNumber copyWithZone:zone];
    copy.uniqueDeviceID = [self.uniqueDeviceID copyWithZone:zone];
    copy.useMacIdentity = self.useMacIdentity;
    return copy;
}

@end

@implementation RelayStatusModel

+ (instancetype)fromDictionary:(NSDictionary *)dict {
    RelayStatusModel *model = [[RelayStatusModel alloc] init];
    model.status = StringOrNil(dict[@"status"]) ?: @"unknown";
    model.errorMessage = StringOrNil(dict[@"error"]);
    model.retryInSecs = NumberOrNil(dict[@"retry_in_secs"]);
    model.liveCode = StringOrNil(dict[@"code"]);
    model.connectedSince = NumberOrNil(dict[@"connected_since"]);
    NSNumber *pidNum = NumberOrNil(dict[@"pid"]);
    model.pid = pidNum ? pidNum.integerValue : 0;
    NSNumber *pendingNum = NumberOrNil(dict[@"pending_restart"]);
    model.pendingRestart = pendingNum ? pendingNum.boolValue : NO;

    id configObj = dict[@"config"];
    if ([configObj isKindOfClass:[NSDictionary class]]) {
        model.config = [RelayConfigModel fromDictionary:(NSDictionary *)configObj];
    } else {
        model.config = [[RelayConfigModel alloc] init];
    }
    return model;
}

@end

@interface RelayClient ()
@property (nonatomic, strong) NSURLSession *session;
@end

@implementation RelayClient

+ (instancetype)sharedClient {
    static RelayClient *shared = nil;
    static dispatch_once_t onceToken;
    dispatch_once(&onceToken, ^{
        shared = [[RelayClient alloc] initWithBaseURL:kDefaultBaseURL];
    });
    return shared;
}

- (instancetype)initWithBaseURL:(NSString *)baseURL {
    self = [super init];
    if (self) {
        _baseURL = [baseURL copy];
        NSURLSessionConfiguration *cfg = [NSURLSessionConfiguration ephemeralSessionConfiguration];
        cfg.timeoutIntervalForRequest = 4.0;
        cfg.timeoutIntervalForResource = 6.0;
        cfg.requestCachePolicy = NSURLRequestReloadIgnoringLocalCacheData;
        _session = [NSURLSession sessionWithConfiguration:cfg];
    }
    return self;
}

- (void)sendRequest:(NSString *)method
               path:(NSString *)path
               body:(NSDictionary * _Nullable)body
         completion:(void (^)(NSDictionary * _Nullable json, NSError * _Nullable error))completion {
    NSURL *url = [NSURL URLWithString:[self.baseURL stringByAppendingString:path]];
    NSMutableURLRequest *req = [NSMutableURLRequest requestWithURL:url];
    req.HTTPMethod = method;
    [req setValue:@"application/json" forHTTPHeaderField:@"Accept"];

    if (body != nil) {
        NSError *encodeErr = nil;
        NSData *data = [NSJSONSerialization dataWithJSONObject:body options:0 error:&encodeErr];
        if (encodeErr) {
            dispatch_async(dispatch_get_main_queue(), ^{
                completion(nil, encodeErr);
            });
            return;
        }
        req.HTTPBody = data;
        [req setValue:@"application/json" forHTTPHeaderField:@"Content-Type"];
    }

    NSURLSessionDataTask *task = [self.session dataTaskWithRequest:req
                                                 completionHandler:^(NSData * _Nullable data,
                                                                     NSURLResponse * _Nullable response,
                                                                     NSError * _Nullable error) {
        if (error) {
            dispatch_async(dispatch_get_main_queue(), ^{
                completion(nil, error);
            });
            return;
        }

        NSHTTPURLResponse *httpResp = (NSHTTPURLResponse *)response;
        NSDictionary *json = nil;
        if (data.length > 0) {
            id parsed = [NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
            if ([parsed isKindOfClass:[NSDictionary class]]) {
                json = (NSDictionary *)parsed;
            }
        }

        if (httpResp.statusCode < 200 || httpResp.statusCode >= 300) {
            NSString *msg = StringOrNil(json[@"error"]) ?: [NSString stringWithFormat:@"HTTP %ld", (long)httpResp.statusCode];
            NSError *apiErr = [NSError errorWithDomain:kRelayClientErrorDomain
                                                  code:httpResp.statusCode
                                              userInfo:@{NSLocalizedDescriptionKey: msg}];
            dispatch_async(dispatch_get_main_queue(), ^{
                completion(nil, apiErr);
            });
            return;
        }

        dispatch_async(dispatch_get_main_queue(), ^{
            completion(json ?: @{}, nil);
        });
    }];
    [task resume];
}

- (void)fetchStatusWithCompletion:(void (^)(RelayStatusModel * _Nullable, NSError * _Nullable))completion {
    [self sendRequest:@"GET" path:@"/status" body:nil completion:^(NSDictionary * _Nullable json, NSError * _Nullable error) {
        if (error || !json) {
            completion(nil, error);
            return;
        }
        completion([RelayStatusModel fromDictionary:json], nil);
    }];
}

- (void)saveConfig:(RelayConfigModel *)config
        completion:(void (^)(RelayConfigModel * _Nullable, NSError * _Nullable))completion {
    [self sendRequest:@"PUT" path:@"/config" body:[config toDictionary] completion:^(NSDictionary * _Nullable json, NSError * _Nullable error) {
        if (error || !json) {
            completion(nil, error);
            return;
        }
        id cfgObj = json[@"config"];
        RelayConfigModel *saved = [cfgObj isKindOfClass:[NSDictionary class]]
            ? [RelayConfigModel fromDictionary:(NSDictionary *)cfgObj]
            : config;
        completion(saved, nil);
    }];
}

- (void)restartDaemonWithCompletion:(void (^)(BOOL, NSError * _Nullable))completion {
    [self sendRequest:@"POST" path:@"/restart" body:nil completion:^(NSDictionary * _Nullable json, NSError * _Nullable error) {
        if (error) {
            completion(NO, error);
            return;
        }
        completion(YES, nil);
    }];
}

@end
