import {
  PLATFORM_DIRECTORY,
  PlatformDirectory,
  PlatformKey,
} from '../data/appDirectory'

export const DEFAULT_PLATFORM: PlatformKey = 'ios'

export const getPlatformDirectory = (
  platform: PlatformKey,
): PlatformDirectory =>
  PLATFORM_DIRECTORY.find((item) => item.id === platform) || PLATFORM_DIRECTORY[0]

export const platformTabs = PLATFORM_DIRECTORY.map((platform) => ({
  id: platform.id,
  label: platform.label,
}))
