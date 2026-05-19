designfilt_iir_cheby1_scale = designfilt("lowpassiir", "FilterOrder", 2, "PassbandFrequency", 0.5, "PassbandRipple", 1, "DesignMethod", "cheby1", "ScalePassband", false);
[designfilt_iir_cheby1_scale_b, designfilt_iir_cheby1_scale_a] = tf(designfilt_iir_cheby1_scale);
designfilt_iir_cheby1_scaled = designfilt("lowpassiir", "FilterOrder", 2, "PassbandFrequency", 0.5, "PassbandRipple", 1, "DesignMethod", "cheby1");
[designfilt_iir_cheby1_scaled_b, designfilt_iir_cheby1_scaled_a] = tf(designfilt_iir_cheby1_scaled);

designfilt_iir_cheby2_scale = designfilt("highpassiir", "FilterOrder", 2, "StopbandFrequency", 0.5, "StopbandAttenuation", 20, "DesignMethod", "cheby2", "ScalePassband", false);
[designfilt_iir_cheby2_scale_b, designfilt_iir_cheby2_scale_a] = tf(designfilt_iir_cheby2_scale);
designfilt_iir_cheby2_scaled = designfilt("highpassiir", "FilterOrder", 2, "StopbandFrequency", 0.5, "StopbandAttenuation", 20, "DesignMethod", "cheby2");
[designfilt_iir_cheby2_scaled_b, designfilt_iir_cheby2_scaled_a] = tf(designfilt_iir_cheby2_scaled);

clear designfilt_iir_cheby1_scale designfilt_iir_cheby1_scaled designfilt_iir_cheby2_scale designfilt_iir_cheby2_scaled;
