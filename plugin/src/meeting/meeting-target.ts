type MeetingTargetOptions<T> = {
  currentMeeting: T | null;
  activeMeetingCandidate: T | null;
  storedMeetingCandidate: T | null;
  isVerifiedMeeting: (candidate: T) => Promise<boolean>;
};

export async function resolveMeetingTarget<T>(options: MeetingTargetOptions<T>): Promise<T | null> {
  if (options.currentMeeting) {
    return options.currentMeeting;
  }
  if (options.activeMeetingCandidate && await options.isVerifiedMeeting(options.activeMeetingCandidate)) {
    return options.activeMeetingCandidate;
  }
  if (options.storedMeetingCandidate && await options.isVerifiedMeeting(options.storedMeetingCandidate)) {
    return options.storedMeetingCandidate;
  }
  return null;
}
