#import "AppDelegate.h"
#import "RelayViewController.h"

@implementation AppDelegate

- (BOOL)application:(UIApplication *)application
    didFinishLaunchingWithOptions:(NSDictionary *)launchOptions {
    self.window = [[UIWindow alloc] initWithFrame:[UIScreen mainScreen].bounds];
    RelayViewController *rootVC = [[RelayViewController alloc] init];
    UINavigationController *nav = [[UINavigationController alloc] initWithRootViewController:rootVC];
    if ([nav.navigationBar respondsToSelector:@selector(setPrefersLargeTitles:)]) {
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wunguarded-availability-new"
        nav.navigationBar.prefersLargeTitles = YES;
#pragma clang diagnostic pop
    }
    self.window.rootViewController = nav;
    [self.window makeKeyAndVisible];
    return YES;
}

@end
