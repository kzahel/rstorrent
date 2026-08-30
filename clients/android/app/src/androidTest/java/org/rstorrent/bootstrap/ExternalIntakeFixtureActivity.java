package org.rstorrent.bootstrap;

import android.app.Activity;
import android.content.ComponentName;
import android.content.Intent;
import android.net.Uri;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;

public final class ExternalIntakeFixtureActivity extends Activity {
    public static final String EXTRA_TARGET_PACKAGE = "target_package";
    public static final String EXTRA_FIXTURE = "fixture";
    public static final String EXTRA_MIME_TYPE = "mime_type";
    public static final String EXTRA_MAGNET = "magnet";
    public static final String EXTRA_PAYLOAD_BASE64 = "payload_base64";
    public static final String EXTRA_REPEAT = "repeat_count";

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        String targetPackage = getIntent().getStringExtra(EXTRA_TARGET_PACKAGE);
        if (targetPackage == null) {
            throw new IllegalArgumentException("target package is required");
        }
        ComponentName target =
                new ComponentName(targetPackage, "org.rstorrent.bootstrap.MainActivity");
        String magnet = getIntent().getStringExtra(EXTRA_MAGNET);
        Intent external;
        if (magnet != null) {
            external = new Intent(Intent.ACTION_VIEW, Uri.parse(magnet));
            external.setComponent(target);
        } else {
            String fixture = getIntent().getStringExtra(EXTRA_FIXTURE);
            if (fixture == null) {
                fixture = ExternalIntakeFixtureProvider.VALID;
            }
            Uri uri =
                    Uri.parse(
                            "content://"
                                    + getPackageName()
                                    + ".external-intake-fixture/"
                                    + fixture);
            String payload = getIntent().getStringExtra(EXTRA_PAYLOAD_BASE64);
            if (payload != null) {
                Bundle extras = new Bundle();
                extras.putString("payload_base64", payload);
                getContentResolver().call(uri, "configure", null, extras);
            }
            external = new Intent(Intent.ACTION_VIEW);
            external.setComponent(target);
            external.setDataAndType(uri, getIntent().getStringExtra(EXTRA_MIME_TYPE));
            external.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
        }
        external.addFlags(
                Intent.FLAG_ACTIVITY_NEW_TASK
                        | Intent.FLAG_ACTIVITY_CLEAR_TOP
                        | Intent.FLAG_ACTIVITY_SINGLE_TOP);
        startActivity(external);
        if (getIntent().getIntExtra(EXTRA_REPEAT, 1) > 1) {
            new Handler(Looper.getMainLooper())
                    .postDelayed(
                            () -> {
                                startActivity(new Intent(external));
                                finish();
                            },
                            500L);
        } else {
            finish();
        }
    }
}
