designfilt_iir_scale = designfilt("bandstopiir", "FilterOrder", 1, "HalfPowerFrequency1", 1, "HalfPowerFrequency2", 3, "SampleRate", 8, "ScalePassband", false);
[designfilt_iir_scale_b, designfilt_iir_scale_a] = tf(designfilt_iir_scale);
