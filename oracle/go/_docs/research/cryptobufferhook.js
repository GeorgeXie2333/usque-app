Java.perform(function () {
    var moduleName = "libwarp_mobile.so";

    var module = null;
    while (!module) {
        console.log("[*] Looking for linker64 module...");
        module = Module.findExportByName("libdl.so", "dlopen");
    }

    console.log("[*] Waiting for " + moduleName + " to be loaded...");

    Interceptor.attach(module, {
        onEnter: function (args) {
            var libName = Memory.readUtf8String(args[0]);
            if (libName.indexOf(moduleName) !== -1) {
                console.log("[*] " + moduleName + " is being loaded...");
            }
        },
        onLeave: function (retval) {
            if (retval.toInt32() !== 0) {
                setTimeout(hookFunction, 1000);
            }
        }
    });

    function hookFunction() {
        var baseAddr = Module.findBaseAddress(moduleName);
        if (!baseAddr) {
            console.log("[-] Failed to find base address for " + moduleName);
            return;
        }
        console.log("[+] Found " + moduleName + " at " + baseAddr);

        var functionOffset = 0x50c508;
        var targetFunction = baseAddr.add(functionOffset);

        /* __int64 __fastcall CRYPTO_BUFFER_new(__int64 a1, __int64 a2, __int64 a3)
{
  return crypto_buffer_new(a1, a2, 0LL, a3);
}*/

        console.log("[*] Hooking CRYPTO_BUFFER_new at " + targetFunction);

        Interceptor.attach(targetFunction, {
            onEnter: function (args) {
                console.log("[*] CRYPTO_BUFFER_new called with args:");
                console.log("    a1: " + args[0]);
                console.log("    a2: " + args[1]);
                console.log("    a3: " + args[2]);

                /* OPENSSL_EXPORT CRYPTO_BUFFER *CRYPTO_BUFFER_new(const uint8_t *data, size_t len,
                                                CRYPTO_BUFFER_POOL *pool)*/


                var data = Memory.readByteArray(args[0], args[1].toInt32());
                var dataUint8 = new Uint8Array(data);
                var dataHex = "";
                for (var i = 0; i < dataUint8.length; i++) {
                    dataHex += ("0" + dataUint8[i].toString(16)).substr(-2);
                }
                console.log("    dataHex: " + dataHex);
                console.log("    data: " + data);

            },
            onLeave: function (retval) {
                console.log("[*] CRYPTO_BUFFER_new returned: " + retval);
            }
        });
    }
});